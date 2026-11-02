use crate::config::Config;
use crate::output::result;
use crate::output::Result;
use crate::state::State;
use serde_json::{json, Value};

pub fn platform_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

pub fn data_root(config: &Config) -> String {
    if !config.data_root.is_empty() {
        return config.data_root.clone();
    }
    if cfg!(target_os = "windows") {
        return std::env::var("USERPROFILE")
            .map(|home| format!("{home}\\Documents\\WXWork"))
            .unwrap_or_default();
    }
    if cfg!(target_os = "macos") {
        return std::env::var("HOME")
            .map(|home| format!("{home}/Library/Containers/com.tencent.WeWorkMac/Data/Library/Application Support/WXWork"))
            .unwrap_or_default();
    }
    String::new()
}

pub fn doctor(config: &Config, state: &State) -> Result<(String, Value, String)> {
    let root = data_root(config);
    let installed = if cfg!(target_os = "windows") {
        std::path::Path::new(&root).exists()
    } else if cfg!(target_os = "macos") {
        std::path::Path::new("/Applications/企业微信.app").exists()
    } else {
        false
    };
    let data = json!({
        "platform": platform_name(),
        "client_installed": installed,
        "client_running": false,
        "memory_access_ok": false,
        "state_dir": state.root,
        "data_root": root,
        "silk_decoder": !config.silk_decoder.is_empty(),
        "accounts": [],
    });
    Ok(result("doctor", data))
}

pub fn accounts(
    _config: &Config,
    _state: &State,
    _verbose: bool,
) -> Result<(String, Value, String)> {
    Ok(result("accounts", json!({"items": []})))
}

pub fn require_local_data() -> Result<()> {
    if !crate::platforms::is_supported() {
        return Err(crate::output::OwcError::precondition(
            "WXWork local data access is unsupported on this platform.",
        ));
    }
    Ok(())
}
