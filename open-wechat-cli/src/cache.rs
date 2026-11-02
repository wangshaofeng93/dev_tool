use crate::output::{OwcError, Result};
use rusqlite::{Connection, OpenFlags};
use std::path::Path;

pub fn open_read_only(path: &Path) -> Result<Connection> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|error| OwcError::data(format!("Cannot open {}: {error}", path.display())))
}

pub fn resolve_db_dir(
    explicit: Option<&Path>,
    account: Option<&str>,
    state: &crate::state::State,
) -> Result<std::path::PathBuf> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    let account = account
        .filter(|value| !value.is_empty())
        .ok_or_else(|| OwcError::precondition("No account selected. Use --account <corp_id>."))?;
    crate::host::require_local_data()?;
    Ok(state.cache_path(account).join("decrypted").join("Data"))
}
