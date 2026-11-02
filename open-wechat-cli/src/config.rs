use crate::output::{OwcError, Result};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub default_account: String,
    pub tz: String,
    pub data_root: String,
    pub silk_decoder: String,
    pub disable_update_check: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            default_account: String::new(),
            tz: "local".to_owned(),
            data_root: String::new(),
            silk_decoder: String::new(),
            disable_update_check: false,
        }
    }
}

pub fn load() -> Result<Config> {
    let state = crate::state::State::from_env()?;
    let path = state.root.join("config.json");
    let mut config = if path.exists() {
        let content = fs::read_to_string(&path).map_err(|error| {
            OwcError::usage(format!(
                "Cannot read configuration {}: {error}",
                path.display()
            ))
        })?;
        serde_json::from_str::<Config>(&content)
            .map_err(|error| OwcError::usage(format!("Invalid config.json: {error}")))?
    } else {
        Config::default()
    };
    if let Ok(value) = std::env::var("OWC_WECOM_DATA") {
        config.data_root = value;
    }
    if let Ok(value) = std::env::var("OWC_SILK_DECODER") {
        config.silk_decoder = value;
    }
    if std::env::var_os("OWC_NO_UPDATE_CHECK").is_some() {
        config.disable_update_check = true;
    }
    Ok(config)
}
