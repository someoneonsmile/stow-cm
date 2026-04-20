use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::anyhow;
use merge::option::with_recurse_strategy;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use crate::constants::{
    CONFIG_FILE_NAME, DEFAULT_CRYPT_ALG, DEFAULT_DECRYPT_LEFT_BOUNDARY,
    DEFAULT_DECRYPT_RIGHT_BOUNDARY, DEFAULT_PACK_DECRYPT, DEFAULT_PACK_TARGET, GLOBAL_CONFIG_FILE,
    GLOBAL_XDG_CONFIG_FILE,
};
use crate::error::Result;
use crate::merge::{Finalize, Merge};
use crate::symlink::SymlinkMode;
use crate::util;

/// pack config
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Merge)]
#[merge(strategy = merge::option::overwrite_none)]
pub struct Config {
    /// symlink mode
    #[serde(rename = "mode")]
    pub symlink_mode: Option<SymlinkMode>,

    /// install to target dir
    pub target: Option<PathBuf>,

    /// ignore file regx
    #[merge(strategy = with_recurse_strategy(merge::vec::append))]
    pub ignore: Option<Vec<String>>,

    /// override file regx
    #[serde(rename = "override")]
    #[merge(strategy = with_recurse_strategy(merge::vec::append))]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Merge)]
#[merge(strategy = merge::option::overwrite_none)]
pub struct EncryptedConfig {
    /// enable default to false
    pub enable: Option<bool>,
    /// decrypted file path when install, default path is ${`XDG_DATA_HOME`:-~/.local/share}/stow-cm/${pack_name}/decrypted/
    pub decrypted_path: Option<PathBuf>,
    /// left boundary of content to be decrypted
    pub left_boundry: Option<String>,
    /// right boundary of content to be decrypted
    pub right_boundry: Option<String>,
    /// the algorithm of encrypted content, default to chacha20poly1305
    pub encrypted_alg: Option<String>,
    /// the algorithm of encrypted content, default to chacha20poly1305
    pub key_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "content")]
pub enum Command {
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
    pub fn from_path<P: AsRef<Path>>(config_path: P) -> Result<Option<Config>> {
        let config_path = util::shell_expand_full(config_path)?;
        if !config_path.exists() {
            return Ok(None);
        }
        let config_str = fs::read_to_string(config_path)?;
        let config: Config = toml::from_str(&config_str)?;
        Ok(Some(config))
    }

    /// get global config
    pub fn global() -> Result<Config> {
        let global_config = Config::from_path(GLOBAL_CONFIG_FILE)?;
        let mut global_xdg_config = Config::from_path(GLOBAL_XDG_CONFIG_FILE)?;
        merge::option::recurse(&mut global_xdg_config, global_config);
        merge::option::recurse(&mut global_xdg_config, Some(Config::default()));
        global_xdg_config.ok_or_else(|| unreachable!("the global config should always return"))
    }

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

impl Command {
    pub async fn exec_async<I, K, V>(&self, wd: impl AsRef<Path>, envs: I) -> Result<()>
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

impl Finalize for Config {
    fn finalize(&mut self) {
        self.target.finalize();
        self.ignore.finalize();
        self.over.finalize();
        self.encrypted.finalize();
    }
}

impl Finalize for EncryptedConfig {
    fn finalize(&mut self) {
        self.decrypted_path.finalize();
        self.left_boundry.finalize();
        self.right_boundry.finalize();
        self.encrypted_alg.finalize();
        self.key_path.finalize();
    }
}

impl Finalize for Option<EncryptedConfig> {
    fn finalize(&mut self) {
        if let Some(inner) = self {
            inner.finalize();
        }
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

#[cfg(test)]
mod test {
    use merge::Merge;

    use super::{Config, EncryptedConfig};
    use crate::merge::Finalize;
    use crate::symlink::SymlinkMode;

    #[test]
    fn config_merge() {
        let mut left = Config {
            symlink_mode: Some(SymlinkMode::Symlink),
            target: Some("temp_a".into()),
            ignore: Some(vec!["a".to_owned()]),
            over: None,
            fold: Some(true),
            init: None,
            clear: None,
            encrypted: None,
        };
        let right = Config {
            symlink_mode: Some(SymlinkMode::Copy),
            target: Some("temp_b".into()),
            ignore: Some(vec!["b".to_owned()]),
            over: Some(vec!["a".to_owned()]),
            fold: Some(true),
            init: None,
            clear: None,
            encrypted: Some(EncryptedConfig::default()),
        };

        left.merge(right);
        assert_eq!(
            Config {
                symlink_mode: Some(SymlinkMode::Symlink),
                target: Some("temp_a".into()),
                ignore: Some(vec!["a".to_owned(), "b".to_owned()]),
                over: Some(vec!["a".to_owned()]),
                fold: Some(true),
                init: None,
                clear: None,
                encrypted: Some(EncryptedConfig::default()),
            },
            left
        );
    }

    fn make_config(target: Option<&str>, ignore: Option<Vec<&str>>) -> Config {
        Config {
            symlink_mode: None,
            target: target.map(Into::into),
            ignore: ignore.map(|v| v.into_iter().map(String::from).collect()),
            over: None,
            fold: None,
            init: None,
            clear: None,
            encrypted: None,
        }
    }

    #[test]
    fn finalize_unset_target() {
        let mut config = make_config(Some("!"), None);
        config.finalize();
        assert_eq!(config.target, None);
    }

    #[test]
    fn finalize_keeps_normal_target() {
        let mut config = make_config(Some("~/.config/test"), None);
        config.finalize();
        assert_eq!(config.target, Some("~/.config/test".into()));
    }

    #[test]
    fn finalize_unset_target_survives_merge() {
        // pack: target = "!" → 应穿透 merge，最终被 finalize 置 None
        let mut pack = Some(make_config(Some("!"), None));
        let global = Some(make_config(Some("~/.config/nvim"), None));
        merge::option::recurse(&mut pack, global);
        let mut config = pack.unwrap();
        config.finalize();
        assert_eq!(config.target, None);
    }

    #[test]
    fn finalize_array_truncate_override() {
        // pack: ['a', '!'] + global: ['x', 'y'] → 合并后 ['a', '!', 'x', 'y'] → 截断 → ['a']
        let mut pack = Some(make_config(None, Some(vec!["a", "!"])));
        let global = Some(make_config(None, Some(vec!["x", "y"])));
        merge::option::recurse(&mut pack, global);
        let mut config = pack.unwrap();
        config.finalize();
        assert_eq!(config.ignore, Some(vec!["a".to_owned()]));
    }

    #[test]
    fn finalize_array_truncate_clear_all() {
        // pack: ['!'] + global: ['x'] → 合并后 ['!', 'x'] → 截断 → [] → None
        let mut pack = Some(make_config(None, Some(vec!["!"])));
        let global = Some(make_config(None, Some(vec!["x"])));
        merge::option::recurse(&mut pack, global);
        let mut config = pack.unwrap();
        config.finalize();
        assert_eq!(config.ignore, None);
    }

    #[test]
    fn finalize_array_no_marker_merges_normally() {
        let mut pack = Some(make_config(None, Some(vec!["a"])));
        let global = Some(make_config(None, Some(vec!["b"])));
        merge::option::recurse(&mut pack, global);
        let config = pack.unwrap();
        assert_eq!(config.ignore, Some(vec!["a".to_owned(), "b".to_owned()]));
    }
}
