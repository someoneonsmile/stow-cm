use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, anyhow, bail};
use merge::option::with_recurse_strategy;
use regex::RegexSet;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

use stow_cm_macros::Finalize;

use crate::base64;
use crate::constants::{
    CONFIG_FILE_NAME, DEFAULT_CRYPT_ALG, DEFAULT_DECRYPT_LEFT_BOUNDARY,
    DEFAULT_DECRYPT_RIGHT_BOUNDARY,
};
use crate::error::Result;
use crate::merge::{Finalize, Merge, SystemInstance};
use crate::paths::{
    default_pack_decrypt, default_pack_target, global_config_path, global_xdg_config_path,
};
use crate::symlink::SymlinkMode;
use crate::util;

/// pack config
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Merge, Finalize)]
#[merge(strategy = merge::option::overwrite_none)]
pub struct Config {
    /// symlink mode
    #[serde(rename = "mode")]
    #[finalize(skip)]
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
    #[finalize(skip)]
    pub fold: Option<bool>,

    /// init script (option)
    #[finalize(skip)]
    pub init: Option<Command>,

    /// clear script (option)
    #[finalize(skip)]
    pub clear: Option<Command>,

    /// encrypted config
    pub encrypted: Option<EncryptedConfig>,
}

/// encrypted config
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Merge, Finalize)]
#[merge(strategy = merge::option::overwrite_none)]
pub struct EncryptedConfig {
    /// enable default to false
    #[finalize(skip)]
    pub enable: Option<bool>,
    /// decrypted file path when install, default path is ${`XDG_STATE_HOME`:-~/.local/state}/stow-cm/${pack_name}/decrypted/
    pub decrypted_path: Option<PathBuf>,
    /// left boundary of content to be decrypted
    #[serde(alias = "left_boundry")]
    pub left_boundary: Option<String>,
    /// right boundary of content to be decrypted
    #[serde(alias = "right_boundry")]
    pub right_boundary: Option<String>,
    /// the algorithm of encrypted content, default to chacha20poly1305
    pub encrypted_alg: Option<String>,
    /// the algorithm of encrypted content, default to chacha20poly1305
    pub key_path: Option<PathBuf>,
}

/// 解析后的加密参数，由 [`EncryptedConfig::resolve`] 一次性生成
pub struct EncryptedParams<'a> {
    pub key: Vec<u8>,
    pub left_boundary: &'a str,
    pub right_boundary: &'a str,
    pub encrypted_alg: &'a str,
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
        let global_config = Config::from_path(global_config_path())?;
        let mut global_xdg_config = Config::from_path(global_xdg_config_path())?;
        merge::option::recurse(&mut global_xdg_config, global_config);
        merge::option::recurse(&mut global_xdg_config, Some(Config::default()));
        global_xdg_config.ok_or_else(|| anyhow!("failed to load global config"))
    }

    /// Normalize config by finalizing and merging system defaults.
    /// First processes "!" markers, then merges system instance values.
    pub fn normalize(&mut self) {
        self.finalize();
        self.merge(Config::system());
    }

    /// 从 `self.ignore` 构造 `RegexSet`，消除 `command.rs` 和 `crypto_process` 中的重复构造逻辑。
    pub fn ignore_regex(&self) -> crate::error::Result<Option<RegexSet>> {
        self.ignore
            .as_ref()
            .map(RegexSet::new)
            .transpose()
            .with_context(|| anyhow!("{:?}", self.ignore))
    }

    /// 从 `self.over` 构造 `RegexSet`，消除 `command.rs` 中的重复构造逻辑。
    pub fn over_regex(&self) -> crate::error::Result<Option<RegexSet>> {
        self.over
            .as_ref()
            .map(RegexSet::new)
            .transpose()
            .with_context(|| anyhow!("{:?}", self.over))
    }
}

impl Default for Config {
    fn default() -> Config {
        Config {
            symlink_mode: Some(SymlinkMode::Symlink),
            target: Some(default_pack_target().into()),
            ignore: None,
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
            Self::Bin(path) => tokio::process::Command::new(path.as_os_str()),

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
                let mut child = c.spawn()?;
                child
                    .stdin
                    .take()
                    .ok_or_else(|| anyhow!("open sh error"))?
                    .write_all(content.as_bytes())
                    .await?;
                // stdin 句柄在 write_all 完成后自动 drop，EOF 已发送
                child.wait().await?;
                return Ok(());
            }
        };
        command.current_dir(wd).envs(envs).status().await?;
        Ok(())
    }
}

impl EncryptedConfig {
    /// 一次性解析所有加密参数（含密钥文件读取），消除 `command.rs` 中的重复提取逻辑
    pub async fn resolve(&self, pack_name: &str) -> Result<EncryptedParams<'_>> {
        let key_path = self
            .key_path
            .as_ref()
            .ok_or_else(|| anyhow!("{pack_name}: key_path is not configured"))?;
        if !tokio::fs::try_exists(key_path).await? {
            bail!("{pack_name}: key_path not exist");
        }
        let key_base64 = tokio::fs::read_to_string(key_path).await.with_context(|| {
            format!(
                "{pack_name}: failed to read from key_path={}",
                key_path.display()
            )
        })?;
        let key = base64::decode(&key_base64)?;

        let left_boundary = self
            .left_boundary
            .as_ref()
            .ok_or_else(|| anyhow!("{pack_name}: left_boundary is not configured"))?
            .as_str();

        let right_boundary = self
            .right_boundary
            .as_ref()
            .ok_or_else(|| anyhow!("{pack_name}: right_boundary is not configured"))?
            .as_str();

        let encrypted_alg = self
            .encrypted_alg
            .as_ref()
            .ok_or_else(|| anyhow!("{pack_name}: encrypted_alg is not configured"))?
            .as_str();

        Ok(EncryptedParams {
            key,
            left_boundary,
            right_boundary,
            encrypted_alg,
        })
    }
}

impl Default for EncryptedConfig {
    fn default() -> Self {
        EncryptedConfig {
            enable: Some(false),
            decrypted_path: Some(default_pack_decrypt().into()),
            left_boundary: Some(DEFAULT_DECRYPT_LEFT_BOUNDARY.into()),
            right_boundary: Some(DEFAULT_DECRYPT_RIGHT_BOUNDARY.into()),
            encrypted_alg: Some(DEFAULT_CRYPT_ALG.into()),
            key_path: None,
        }
    }
}

impl SystemInstance for Config {
    fn system() -> Self {
        Config {
            symlink_mode: None,
            target: None,
            ignore: Some(vec![CONFIG_FILE_NAME.to_string()]),
            over: None,
            fold: None,
            init: None,
            clear: None,
            encrypted: None,
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
