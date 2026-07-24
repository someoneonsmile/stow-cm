use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::symlink::{Symlink, SymlinkMode};

/// track struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    /// save links
    pub links: Vec<Symlink>,
    /// decrypted file path
    pub decrypted_path: Option<PathBuf>,
    /// 是否开启了加解密
    #[serde(default)]
    pub encrypted: bool,
    /// pack 名称（安装时记录，供 `list` 等命令使用）
    #[serde(default)]
    pub pack_name: Option<String>,
    /// pack 原始路径（安装时记录，供 `list` 等命令使用）
    #[serde(default)]
    pub pack_path: Option<PathBuf>,
    /// 安装目标目录（安装时记录，供 `list` 等命令使用）
    #[serde(default)]
    pub target: Option<PathBuf>,
    /// 安装时的 symlink 模式（供 reload 去重判断使用）
    #[serde(default)]
    pub symlink_mode: Option<SymlinkMode>,
}
