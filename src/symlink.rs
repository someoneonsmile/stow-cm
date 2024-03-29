use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Symlink {
    /// the path will link to
    pub src: PathBuf,
    /// the path of the link file
    pub dst: PathBuf,
}

impl Symlink {
    pub(crate) async fn create(&self) -> Result<()> {
        if let Some(parent) = self.dst.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::symlink(&self.src, &self.dst)
            .await
            .with_context(|| format!("symlink: {self:?}"))?;
        Ok(())
    }

    pub(crate) async fn remove(&self) -> Result<()> {
        fs::remove_file(&self.dst)
            .await
            .with_context(|| format!("symlink: {:?}", self))?;
        Ok(())
    }
}
