use log::info;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;

use crate::constants::*;
use crate::error::Result;
use crate::merge::Merge;
use crate::util;

/// pack config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Config {
    /// install to target dir
    pub target: Option<PathBuf>,

    /// ignore file regx
    pub ignore: Option<Vec<String>>,

    /// override file regx
    #[serde(rename = "override")]
    pub over: Option<Vec<String>>,

    /// force override
    pub fold: Option<bool>,

    /// init script (option)
    pub init: Option<Command>,

    /// clear script (option)
    pub clear: Option<Command>,

    /// is it the decrypted link
    pub decrypted: bool,

    /// decrypted file path
    pub decrypted_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "content")]
pub(crate) enum Command {
    /// 可执行文件/脚本
    Bin(PathBuf),

    /// shell 脚本
    Shell(PathBuf),

    /// 脚本字符串
    ShellStr(String),

    /// Makefile
    Make(PathBuf),

    /// python 脚本
    Python(PathBuf),

    /// lua 脚本
    Lua(PathBuf),
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
        config.init_deal();
        Ok(Some(config))
    }

    /// deal some specical case
    fn init_deal(&mut self) {
        if self
            .target
            .as_ref()
            .is_some_and(|path| matches!(path.to_str().map(str::trim), None | Some(UNSET_VALUE)))
        {
            self.target = None;
        }
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
            target: Some(DEFAULT_PACK_TARGET.into()),
            ignore: Some(vec![CONFIG_FILE_NAME.to_string()]),
            over: None,
            fold: Some(true),
            init: None,
            clear: None,
            // TODO:
            decrypted: false,
            decrypted_path: Some(DEFAULT_PACK_DECRYPT.into()),
        }
    }
}

impl Merge<Self> for Config {
    fn merge(mut self, other: Config) -> Config {
        self.target = self.target.merge(other.target);
        self.ignore = self.ignore.merge(other.ignore);
        self.over = self.over.merge(other.over);
        self.fold = self.fold.merge(other.fold);
        self.init = self.init.merge(other.init);
        self.clear = self.clear.merge(other.clear);
        self
    }
}

impl Command {
    pub(crate) async fn exec_async<I, K, V>(&self, wd: impl AsRef<Path>, envs: I) -> Result<()>
    where
        I: IntoIterator<Item = (K, V)> + Clone,
        K: AsRef<OsStr>,
        V: AsRef<OsStr>,
    {
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

            Self::Lua(path) => {
                let mut c = tokio::process::Command::new("lua");
                c.arg(path.as_os_str());
                c
            }

            Self::ShellStr(content) => {
                let mut c = tokio::process::Command::new("sh");
                c.current_dir(&wd);
                c.envs(envs.clone());
                c.stdin(Stdio::piped());
                c.spawn()?
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(content.as_bytes())
                    .await?;
                c
            }
        };
        command.current_dir(wd).envs(envs).status().await?;
        Ok(())
    }
}

impl Merge<Self> for Command {
    fn merge(self, _other: Self) -> Self {
        self
    }
}
