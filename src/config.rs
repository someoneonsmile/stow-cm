use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{ Path, PathBuf };

use crate::error::StowResult;
use crate::util;

pub static CONFIG_FILE_NAME: &'static str = ".stowrc";

/// pack config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// install to target dir
    pub target: Option<PathBuf>,

    /// ignore file regx
    pub ignore: Option<Vec<String>>,
}

impl Config {
    /// parse config file
    pub fn from_path<P: AsRef<Path>>(config_path: P) -> StowResult<Option<Config>> {
        if !config_path.as_ref().exists() {
            return Ok(None);
        }
        let config_str = fs::read_to_string(config_path.as_ref())?;
        let mut config: Config = toml::from_str(&config_str)?;
        if let Some(target) = config.target {
            config.target = Some(util::shell_expend_tilde(target));
        }
        return Ok(Some(config));
    }
}

impl Default for Config {
    fn default() -> Config {
        Config {
            target: Some(util::shell_expend_tilde("~")),
            ignore: Some(vec![CONFIG_FILE_NAME.to_string()]),
        }
    }
}

