use crate::output::{OwcError, Result};
use std::path::Path;

pub fn read_key(key_hex: Option<&str>, key_file: Option<&Path>) -> Result<Vec<u8>> {
    if let Some(value) = key_hex {
        return hex::decode(value.trim()).map_err(|_| OwcError::usage("Invalid --key-hex value."));
    }
    if let Some(path) = key_file {
        let content = std::fs::read_to_string(path).map_err(|error| {
            let code = if error.kind() == std::io::ErrorKind::PermissionDenied {
                crate::output::ErrorCode::Permission
            } else {
                crate::output::ErrorCode::KeyNotFound
            };
            OwcError::new(
                code,
                format!("Cannot read key file: {error}"),
                "Provide a readable --key-file or use --key-hex.",
            )
        })?;
        let value = serde_json::from_str::<serde_json::Value>(&content)
            .ok()
            .and_then(|json| {
                json.get("key_hex")
                    .and_then(|item| item.as_str())
                    .map(str::to_owned)
            })
            .unwrap_or(content.trim().to_owned());
        return hex::decode(value.trim())
            .map_err(|_| OwcError::usage("Key file does not contain valid hex."));
    }
    if let Ok(value) = std::env::var("OWC_KEY_HEX") {
        return hex::decode(value.trim())
            .map_err(|_| OwcError::usage("Invalid OWC_KEY_HEX value."));
    }
    Err(OwcError::new(
        crate::output::ErrorCode::KeyNotFound,
        "No database key was supplied.",
        "Use `owc key scan --account <corp_id>` or supply --key-file / --key-hex.",
    ))
}
