use crate::cli::{Cli, SnapshotArgs};
use crate::config::Config;
use crate::output::{OwcError, Result};
use crate::state::State;
use rusqlite::backup::Backup;
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;

pub fn run(
    args: SnapshotArgs,
    _cli: &Cli,
    config: &Config,
    state: &State,
) -> Result<(String, Value, String)> {
    crate::host::require_local_data()?;
    let account_dir = crate::host::data_root(config);
    let source = PathBuf::from(account_dir).join(&args.account);
    let out = if args.out.is_absolute() {
        args.out
    } else {
        state.cache_path(&args.account).join(args.out)
    };
    fs::create_dir_all(&out).map_err(|error| OwcError::data(error.to_string()))?;
    let requested = if args.dbs.is_empty() {
        vec![
            "message.db".to_owned(),
            "session.db".to_owned(),
            "user.db".to_owned(),
        ]
    } else {
        args.dbs.clone()
    };
    let mut copied = Vec::new();
    for name in requested {
        let from = source.join(&name);
        if !from.is_file() {
            continue;
        }
        let to = out.join(&name);
        if args.reuse && to.exists() {
            copied.push(json!({"database": name, "path": to, "reused": true}));
            continue;
        }
        // S3: SQLite's read-only backup API is used as the least invasive portable WAL-aware snapshot.
        let source_conn = Connection::open_with_flags(
            &from,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| OwcError::new(
            crate::output::ErrorCode::Snapshot,
            format!("Could not open {} read-only: {error}", from.display()),
            "Close active database writers and retry `owc snapshot --account <corp_id> --out <dir>`.",
        ))?;
        let mut target_conn =
            Connection::open(&to).map_err(|error| OwcError::data(error.to_string()))?;
        let backup = Backup::new(&source_conn, &mut target_conn)
            .map_err(|error| OwcError::data(error.to_string()))?;
        backup
            .run_to_completion(64, std::time::Duration::from_millis(20), None)
            .map_err(|error| {
                OwcError::new(
                    crate::output::ErrorCode::Snapshot,
                    error.to_string(),
                    "Retry with `owc snapshot --account <corp_id> --out <dir>`.",
                )
            })?;
        drop(backup);
        if args.verify {
            let check: String = target_conn
                .query_row("PRAGMA quick_check", [], |row| row.get(0))
                .map_err(|error| OwcError::data(error.to_string()))?;
            if check != "ok" {
                return Err(OwcError::data(format!("Snapshot quick_check: {check}")));
            }
        }
        copied.push(json!({"database": name, "path": to, "reused": false}));
    }
    let _ = args.no_wal;
    Ok(crate::output::result(
        "snapshot",
        json!({"databases": copied}),
    ))
}
