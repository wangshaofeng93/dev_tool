use crate::output::{OwcError, Result};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct State {
    pub root: PathBuf,
}

impl State {
    pub fn from_env() -> Result<Self> {
        if let Some(root) = std::env::var_os("OWC_STATE_DIR") {
            return Ok(Self {
                root: PathBuf::from(root),
            });
        }
        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .ok_or_else(|| OwcError::precondition("Cannot determine the home directory."))?;
        Ok(Self {
            root: home
                .join(".config")
                .join("agent-skills")
                .join("open-wechat-cli"),
        })
    }

    pub fn cache_path(&self, corp_id: &str) -> PathBuf {
        self.root.join("cache").join(corp_id)
    }

    pub fn export_path(&self) -> PathBuf {
        self.root.join("export")
    }
}
