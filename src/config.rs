use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;

use crate::constants::{
    CONFIG_FILE_NAME, DEFAULT_CRYPT_ALG, DEFAULT_DECRYPT_LEFT_BOUNDARY,
    DEFAULT_DECRYPT_RIGHT_BOUNDARY, DEFAULT_PACK_DECRYPT, DEFAULT_PACK_TARGET, GLOBAL_CONFIG_FILE,
    GLOBAL_XDG_CONFIG_FILE, UNSET_VALUE,
};
use crate::error::Result;
use crate::merge::Merge;
use crate::symlink::SymlinkMode;
use crate::util;

/// pack config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Config {
    /// symlink mode
    #[serde(rename = "mode")]
    pub symlink_mode: Option<SymlinkMode>,

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

    /// encrypted config
    pub encrypted: Option<EncryptedConfig>,
}

/// encrypted config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct EncryptedConfig {
    /// enable default to false
    pub(crate) enable: Option<bool>,
    /// decrypted file path when install, default path is ${`XDG_DATA_HOME`:-~/.local/share}/stow-cm/${pack_name}/decrypted/
    pub(crate) decrypted_path: Option<PathBuf>,
    /// left boundary of content to be decrypted
    pub(crate) left_boundry: Option<String>,
    /// right boundary of content to be decrypted
    pub(crate) right_boundry: Option<String>,
    /// the algorithm of encrypted content, default to chacha20poly1305
    pub(crate) encrypted_alg: Option<String>,
    /// the algorithm of encrypted content, default to chacha20poly1305
    pub(crate) key_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "content")]
pub(crate) enum Command {
    /// executable bin / script
    Bin(PathBuf),

    /// shell script
    Shell(PathBuf),

    /// string script
    ShellStr(String),

    /// Makefile
    Make(PathBuf),

    /// python script
    Python(PathBuf),

    /// lua script
    Lua(PathBuf),
}

impl Config {
    /// parse config file
    pub(crate) fn from_path<P: AsRef<Path>>(config_path: P) -> Result<Option<Config>> {
        let config_path = util::shell_expand_full(config_path)?;
        if !config_path.exists() {
            return Ok(None);
        }
        let config_str = fs::read_to_string(config_path)?;
        let mut config: Config = toml::from_str(&config_str)?;
        config.init_deal();
        Ok(Some(config))
    }

    /// get global config
    pub(crate) fn global() -> Result<Config> {
        let global_config = Config::from_path(GLOBAL_CONFIG_FILE)?;
        let global_xdg_config = Config::from_path(GLOBAL_XDG_CONFIG_FILE)?;
        global_xdg_config
            .merge(global_config)
            .merge(Some(Config::default()))
            .ok_or_else(|| unreachable!("the global config should always return"))
    }

    /// deal some special case
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
            symlink_mode: Some(SymlinkMode::Symlink),
            target: Some(DEFAULT_PACK_TARGET.into()),
            ignore: Some(vec![CONFIG_FILE_NAME.to_string()]),
            over: None,
            fold: Some(true),
            init: None,
            clear: None,
            encrypted: Some(EncryptedConfig::default()),
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
        self.encrypted = self.encrypted.merge(other.encrypted);
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
                    .ok_or_else(|| anyhow!("open sh error"))?
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

impl Default for EncryptedConfig {
    fn default() -> Self {
        EncryptedConfig {
            enable: Some(false),
            decrypted_path: Some(DEFAULT_PACK_DECRYPT.into()),
            left_boundry: Some(DEFAULT_DECRYPT_LEFT_BOUNDARY.into()),
            right_boundry: Some(DEFAULT_DECRYPT_RIGHT_BOUNDARY.into()),
            encrypted_alg: Some(DEFAULT_CRYPT_ALG.into()),
            key_path: None,
        }
    }
}

impl Merge<Self> for EncryptedConfig {
    fn merge(self, other: Self) -> Self {
        EncryptedConfig {
            enable: self.enable.merge(other.enable),
            decrypted_path: self.decrypted_path.merge(other.decrypted_path),
            right_boundry: match self.left_boundry {
                Some(_) => self.right_boundry.merge(other.right_boundry),
                None => other.right_boundry,
            },
            left_boundry: self.left_boundry.merge(other.left_boundry),
            encrypted_alg: self.encrypted_alg.merge(other.encrypted_alg),
            key_path: self.key_path.merge(other.key_path),
        }
    }
}
