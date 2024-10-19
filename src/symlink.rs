use anyhow::{anyhow, Context};
use serde::{Deserialize, Serialize};
use std::{fmt::Display, path::PathBuf};
use tokio::fs;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Symlink {
    /// the path will link to
    pub src: PathBuf,
    /// the path of the link file
    pub dst: PathBuf,
}

impl Display for Symlink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} -> {}",
            self.dst.to_string_lossy(),
            self.src.to_string_lossy()
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
        fs::symlink(&self.src, &self.dst)
            .await
            .with_context(|| format!("failed to create symlink: {self}"))?;
        Ok(())
    }

    pub(crate) async fn remove(&self) -> Result<()> {
        if !self.dst.is_symlink() {
            return Err(anyhow!("{} is not symlink", self.dst.to_string_lossy()));
        }
        fs::remove_file(&self.dst)
            .await
            .with_context(|| format!("failed to remove symlink: {self}"))?;
        Ok(())
    }
}
