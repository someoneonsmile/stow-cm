use std::{
    fmt::{Debug, Display},
    path::PathBuf,
};

use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symlink {
    /// the path will link to
    pub src: PathBuf,
    /// the path of the link file
    pub dst: PathBuf,
    /// mode
    #[serde(default)]
    pub mode: SymlinkMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum SymlinkMode {
    #[default]
    #[serde(rename = "symlink")]
    Symlink,
    #[serde(rename = "copy")]
    Copy,
}

impl Display for Symlink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} -> {} [{:?}]",
            self.dst.to_string_lossy(),
            self.src.to_string_lossy(),
            self.mode
        )
    }
}

impl Symlink {
    pub async fn create(&self, force: bool) -> Result<()> {
        if let Some(parent) = self.dst.parent() {
            fs::create_dir_all(parent).await?;
        }

        if force {
            // the dir is empty or override regex matched
            // 用 symlink_metadata 一次性获取元数据，避免多次 stat() 调用之间的 TOCTOU 竞态窗口
            match fs::symlink_metadata(&self.dst).await {
                Ok(meta) => {
                    let ft = meta.file_type();
                    if ft.is_file() || ft.is_symlink() {
                        fs::remove_file(&self.dst).await?;
                    } else if ft.is_dir() {
                        fs::remove_dir_all(&self.dst).await?;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    // 目标不存在，无需清理
                }
                Err(e) => return Err(e.into()),
            }
        }
        self.mode.create(self).await?;
        Ok(())
    }

    pub async fn remove(&self) -> Result<()> {
        self.mode.remove(self).await?;
        Ok(())
    }
}

impl SymlinkMode {
    async fn create(&self, symlink: &Symlink) -> Result<()> {
        match self {
            SymlinkMode::Symlink => {
                fs::symlink(&symlink.src, &symlink.dst)
                    .await
                    .with_context(|| format!("failed to create symlink: {symlink}"))?;
                Ok(())
            }
            SymlinkMode::Copy => {
                fs::copy(&symlink.src, &symlink.dst)
                    .await
                    .with_context(|| format!("failed to create symlink: {symlink}"))?;
                Ok(())
            }
        }
    }

    async fn remove(&self, symlink: &Symlink) -> Result<()> {
        match self {
            SymlinkMode::Symlink => {
                // 用 symlink_metadata 一次性获取元数据，避免多次 stat() 调用之间的 TOCTOU 竞态窗口
                match fs::symlink_metadata(&symlink.dst).await {
                    Ok(meta) => {
                        if meta.file_type().is_symlink() {
                            fs::remove_file(&symlink.dst)
                                .await
                                .with_context(|| format!("failed to remove symlink: {symlink}"))?;
                            Ok(())
                        } else {
                            Err(anyhow!("{} is not symlink", symlink.dst.to_string_lossy()))
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(e) => Err(e.into()),
                }
            }
            SymlinkMode::Copy => {
                fs::remove_file(&symlink.dst)
                    .await
                    .with_context(|| format!("failed to remove symlink: {symlink}"))?;
                Ok(())
            }
        }
    }
}
