use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use std::{
    fmt::{Debug, Display},
    path::PathBuf,
};
use tokio::fs;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Symlink {
    /// the path will link to
    pub src: PathBuf,
    /// the path of the link file
    pub dst: PathBuf,
    /// mode
    #[serde(default)]
    pub mode: SymlinkMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) enum SymlinkMode {
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
    pub(crate) async fn create(&self, force: bool) -> Result<()> {
        if let Some(parent) = self.dst.parent() {
            fs::create_dir_all(parent).await?;
        }

        if force {
            // the dir is empty or override regex matched
            if self.dst.is_file() || self.dst.is_symlink() {
                fs::remove_file(&self.dst).await?;
            } else if self.dst.is_dir() {
                fs::remove_dir_all(&self.dst).await?;
            }
        }
        self.mode.create(self).await?;
        Ok(())
    }

    pub(crate) async fn remove(&self) -> Result<()> {
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
                if !symlink.dst.is_symlink() {
                    if symlink.dst.exists() {
                        return Err(anyhow!("{} is not symlink", symlink.dst.to_string_lossy()));
                    }
                    return Ok(());
                }
                fs::remove_file(&symlink.dst)
                    .await
                    .with_context(|| format!("failed to remove symlink: {symlink}"))?;
                Ok(())
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
