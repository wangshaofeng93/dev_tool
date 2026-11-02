use crate::cli::{Cli, KeyCommand, KeyScanArgs, KeyVerifyArgs};
use crate::config::Config;
use crate::output::{OwcError, Result};
use crate::state::State;
use serde_json::Value;

pub fn run(
    command: KeyCommand,
    _cli: &Cli,
    _config: &Config,
    _state: &State,
) -> Result<(String, Value, String)> {
    match command {
        KeyCommand::Scan(args) => scan(args),
        KeyCommand::Verify(args) => verify(args),
    }
}

fn scan(_args: KeyScanArgs) -> Result<(String, Value, String)> {
    Err(OwcError::precondition(
        "Live process-memory key scanning is not implemented in this reconstruction.",
    ))
}

fn verify(args: KeyVerifyArgs) -> Result<(String, Value, String)> {
    let key = crate::extract::read_key(args.key_hex.as_deref(), args.key_file.as_deref())?;
    if key.len() != 32 {
        return Err(OwcError::usage(
            "Database keys must contain exactly 32 bytes.",
        ));
    }
    Err(OwcError::precondition(format!(
        "No readable encrypted database was found to verify the supplied key for {}.",
        args.account
    )))
}
