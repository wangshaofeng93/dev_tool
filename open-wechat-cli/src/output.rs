use serde::Serialize;

pub type Result<T> = std::result::Result<T, OwcError>;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    Usage,
    Precondition,
    KeyNotFound,
    Permission,
    Dependency,
    Snapshot,
    Data,
    FormatMismatch,
    Internal,
}

#[derive(Debug, Serialize)]
pub struct OwcError {
    pub code: ErrorCode,
    pub message: String,
    pub repair: String,
}

impl OwcError {
    pub fn new(code: ErrorCode, message: impl Into<String>, repair: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            repair: repair.into(),
        }
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Usage, message, "Run `owc --help`.")
    }

    pub fn precondition(message: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::Precondition,
            message,
            "Run `owc doctor --json` and check local platform prerequisites.",
        )
    }

    pub fn data(message: impl Into<String>) -> Self {
        Self::new(
            ErrorCode::Data,
            message,
            "Run `owc schema --json` and report the table/column layout.",
        )
    }

    pub fn exit_code(&self) -> i32 {
        match self.code {
            ErrorCode::Usage => 2,
            ErrorCode::Precondition | ErrorCode::KeyNotFound => 3,
            ErrorCode::Permission => 4,
            ErrorCode::Dependency => 5,
            ErrorCode::Snapshot | ErrorCode::Data | ErrorCode::FormatMismatch => 6,
            ErrorCode::Internal => 70,
        }
    }
}

#[derive(Serialize)]
struct Success<'a> {
    ok: bool,
    command: &'a str,
    data: &'a serde_json::Value,
}

#[derive(Serialize)]
struct Failure<'a> {
    ok: bool,
    error: &'a OwcError,
}

pub fn emit_success(command: &str, data: &serde_json::Value, human: &str, json: bool) {
    if json {
        let envelope = Success {
            ok: true,
            command,
            data,
        };
        match serde_json::to_string(&envelope) {
            Ok(line) => println!("{line}"),
            Err(_) => println!(r#"{{"ok":true,"command":"internal","data":{{}}}}"#),
        }
    } else if !human.is_empty() {
        println!("{human}");
    }
}

pub fn emit_error(error: &OwcError, _json: bool) {
    let envelope = Failure { ok: false, error };
    match serde_json::to_string(&envelope) {
        Ok(line) => eprintln!("{line}"),
        Err(_) => eprintln!(
            r#"{{"ok":false,"error":{{"code":"INTERNAL","message":"serialization failed","repair":"Run `owc --help`."}}}}"#
        ),
    }
}

pub fn result(command: &str, data: serde_json::Value) -> (String, serde_json::Value, String) {
    let human = serde_json::to_string_pretty(&data).unwrap_or_else(|_| "{}".to_owned());
    (command.to_owned(), data, human)
}
