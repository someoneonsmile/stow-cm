use anyhow::Context;
use std::path::PathBuf;
use tokio::fs;

use crate::error::Result;

#[derive(Debug, Clone)]
pub(crate) struct Symlink {
    pub src: PathBuf,
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
