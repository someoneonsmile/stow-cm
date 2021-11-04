use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use anyhow::Result;

use crate::cli::Opt;
use crate::util;

pub static CONFIG_FILE_NAME: &'static str = ".stowrc";

/// pack config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// install to target dir
    pub target: Option<PathBuf>,

    /// ignore file regx
    pub ignore: Option<Vec<String>>,

    /// ignore file regx
    pub force: Option<bool>,
}

impl Config {
    /// parse config file
    pub fn from_path<P: AsRef<Path>>(config_path: P) -> Result<Option<Config>> {
        if !config_path.as_ref().exists() {
            return Ok(None);
        }
        let config_str = fs::read_to_string(config_path.as_ref())?;
        let mut config: Config = toml::from_str(&config_str)?;
        if let Some(target) = config.target {
            config.target = Some(util::shell_expend_full(target)?);
        }
        return Ok(Some(config));
    }

    /// parse config from cli args
    pub fn from_cli(opt: &Opt) -> Result<Option<Config>> {
        Ok(Some(Config {
            target: None,
            ignore: None,
            force: Some(opt.force),
        }))
    }
}

impl Default for Config {
    fn default() -> Config {
        Config {
            target: Some(util::shell_expend_tilde("~")),
            ignore: Some(vec![CONFIG_FILE_NAME.to_string()]),
            force: None,
        }
    }
}
