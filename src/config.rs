use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;

use crate::error::Result;
use crate::merge::Merge;
use crate::util;

pub static CONFIG_FILE_NAME: &str = ".stowrc";

/// pack config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Config {
    /// install to target dir
    pub target: Option<PathBuf>,

    /// ignore file regx
    pub ignore: Option<Vec<String>>,

    /// force override
    pub force: Option<bool>,

    /// init script (option)
    pub init: Option<Command>,

    /// clear script (option)
    pub clear: Option<Command>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "content")]
pub(crate) enum Command {
    /// 可执行文件/脚本
    Bin(PathBuf),

    /// shell 脚本
    Make(PathBuf),

    /// shell 脚本
    Shell(PathBuf),

    /// python 脚本
    Python(PathBuf),

    /// 脚本字符串
    Script(String),
}

impl Config {
    /// parse config file
    pub(crate) fn from_path<P: AsRef<Path>>(config_path: P) -> Result<Option<Config>> {
        let config_path = util::shell_expend_full(config_path)?;
        if !config_path.exists() {
            return Ok(None);
        }
        let config_str = fs::read_to_string(config_path)?;
        let mut config: Config = toml::from_str(&config_str)?;
        if let Some(target) = config.target {
            config.target = Some(util::shell_expend_full(target)?);
        }
        Ok(Some(config.merge(Default::default())))
    }

    // parse config from cli args
    // pub(crate) fn from_cli(opt: &Opt) -> Result<Option<Config>> {
    //     Ok(Some(Config {
    //         force: Some(opt.force),
    //         ..Default::default()
    //     }))
    // }
}

impl Default for Config {
    fn default() -> Config {
        Config {
            ignore: Some(vec![CONFIG_FILE_NAME.to_string()]),
            target: None,
            force: None,
            init: None,
            clear: None,
        }
    }
}

impl Command {
    pub(crate) async fn exec_async(&self, wd: impl AsRef<Path>) -> Result<()> {
        let mut command = match self {
            Self::Bin(path) => {
                let path = fs::canonicalize(PathBuf::from(".").join(path))?;
                let c = tokio::process::Command::new(path.as_os_str());
                c
            }
            Self::Make(path) => {
                let mut c = tokio::process::Command::new("make");
                c.arg(path.as_os_str());
                c
            }
            Self::Shell(path) => {
                let mut c = tokio::process::Command::new("sh");
                c.arg(path.as_os_str());
                c
            }
            Self::Python(path) => {
                let mut c = tokio::process::Command::new("python");
                c.arg(path.as_os_str());
                c
            }

            Self::Script(content) => {
                let mut c = tokio::process::Command::new("sh");
                c.stdin(Stdio::piped())
                    .spawn()?
                    .stdin
                    .take()
                    .unwrap()
                    .write(content.as_bytes())
                    .await?;
                c
            }
        };
        command.current_dir(wd).spawn()?.wait().await?;
        Ok(())
    }
}
