use crate::cipher::decrypt_database;
use crate::cli::{Cli, DecryptArgs};
use crate::config::Config;
use crate::extract::read_key;
use crate::output::{OwcError, Result};
use crate::state::State;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub fn run(
    args: DecryptArgs,
    _cli: &Cli,
    _config: &Config,
    state: &State,
) -> Result<(String, Value, String)> {
    crate::host::require_local_data()?;
    let source = args
        .snapshot
        .unwrap_or_else(|| state.cache_path(&args.account).join("snapshot"));
    let key = read_key(args.key_hex.as_deref(), args.key_file.as_deref())?;
    let files = database_files(&source, &args.dbs)?;
    if files.is_empty() {
        return Err(OwcError::precondition(format!(
            "No database snapshots found in {}.",
            source.display()
        )));
    }
    fs::create_dir_all(&args.out).map_err(|error| OwcError::data(error.to_string()))?;
    let mut written = Vec::new();
    for file in files {
        let encrypted = fs::read(&file).map_err(|error| OwcError::data(error.to_string()))?;
        let decrypted = decrypt_database(&encrypted, &key)?;
        let target = args.out.join(file.file_name().unwrap_or_default());
        let verify_path = if args.verify_only {
            args.out.join(format!(
                ".{}.verify.tmp",
                file.file_name().unwrap_or_default().to_string_lossy()
            ))
        } else {
            target.clone()
        };
        fs::write(&verify_path, &decrypted.bytes)
            .map_err(|error| OwcError::data(error.to_string()))?;
        let verified = crate::cache::open_read_only(&verify_path)?
            .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
            .map_err(|error| OwcError::data(error.to_string()))?;
        if verified != "ok" {
            let _ = fs::remove_file(&verify_path);
            return Err(OwcError::data(format!("Decrypted quick_check: {verified}")));
        }
        if args.verify_only {
            fs::remove_file(&verify_path).map_err(|error| OwcError::data(error.to_string()))?;
        }
        written.push(json!({
            "database": file.file_name().unwrap_or_default().to_string_lossy(),
            "profile": decrypted.profile.name(),
            "verified": verified == "ok",
            "path": target,
        }));
    }
    Ok(crate::output::result(
        "decrypt",
        json!({"databases": written, "verified_only": args.verify_only}),
    ))
}

pub(crate) fn database_files(source: &Path, requested: &[String]) -> Result<Vec<PathBuf>> {
    let names = if requested.is_empty() {
        vec![
            "message.db".to_owned(),
            "session.db".to_owned(),
            "user.db".to_owned(),
        ]
    } else {
        requested.to_vec()
    };
    let mut files = Vec::new();
    for name in names {
        let path = source.join(&name);
        if path.is_file() {
            files.push(path);
        }
    }
    Ok(files)
}
