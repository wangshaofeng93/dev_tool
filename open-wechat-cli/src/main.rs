mod cache;
mod cipher;
mod cli;
mod codec;
mod config;
mod decrypt;
mod extract;
mod host;
mod keyscan;
mod media;
mod output;
mod platforms;
mod proto;
mod query;
mod render;
mod schema;
mod silk;
mod snapshot;
mod state;
mod store;
#[cfg(test)]
mod tests;

use clap::Parser;
use cli::{Cli, Command};
use output::{OwcError, Result};

fn main() {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let kind = error.kind();
            if matches!(
                kind,
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                let _ = error.print();
                std::process::exit(0);
            }
            let err = OwcError::usage(error.to_string());
            output::emit_error(&err, false);
            std::process::exit(err.exit_code());
        }
    };
    let is_json = cli.json;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(cli)));
    match result {
        Ok(Ok((command, data, human))) => {
            output::emit_success(&command, &data, &human, is_json);
        }
        Ok(Err(err)) => {
            output::emit_error(&err, is_json);
            std::process::exit(err.exit_code());
        }
        Err(_) => {
            let err = OwcError::new(
                output::ErrorCode::Internal,
                "An unexpected internal error occurred.",
                "Run `owc --verbose` and report the failure without sharing database keys.",
            );
            output::emit_error(&err, is_json);
            std::process::exit(err.exit_code());
        }
    }
}

fn run(cli: Cli) -> Result<(String, serde_json::Value, String)> {
    let config = config::load()?;
    let state = state::State::from_env()?;
    let Cli {
        json,
        verbose,
        account,
        tz,
        command,
    } = cli;
    let context = Cli {
        json,
        verbose,
        account,
        tz,
        command: Command::Doctor,
    };
    match command {
        Command::Doctor => host::doctor(&config, &state),
        Command::Accounts {
            verbose: account_verbose,
        } => host::accounts(&config, &state, account_verbose || context.verbose),
        Command::Key { command } => keyscan::run(command, &context, &config, &state),
        Command::Snapshot(args) => snapshot::run(args, &context, &config, &state),
        Command::Decrypt(args) => decrypt::run(args, &context, &config, &state),
        Command::Schema(args) => schema::run(args, &context, &config, &state),
        Command::Sessions {
            command: cli::ListCommand::List(args),
        } => store::sessions(args, &context, &config, &state),
        Command::Contacts {
            command: cli::ListCommand::List(args),
        } => store::contacts(args, &context, &config, &state),
        Command::Messages { command } => query::messages(command, &context, &config, &state),
        Command::Media { command } => media::run(command, &context, &config, &state),
        Command::History(args) => query::history(args, &context, &config, &state),
    }
}
