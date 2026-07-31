use std::{
    fmt::{Debug, Display},
    path::{Path, PathBuf},
};

use anyhow::{Context, anyhow};
use serde::{Deserialize, Serialize};

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
    #[serde(rename = "move")]
    Move,
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
    pub fn create(&self, force: bool) -> Result<()> {
        if let Some(parent) = self.dst.parent() {
            std::fs::create_dir_all(parent)?;
        }

        if force {
            // the dir is empty or override regex matched
            // 用 symlink_metadata 一次性获取元数据，避免多次 stat() 调用之间的 TOCTOU 竞态窗口
            match std::fs::symlink_metadata(&self.dst) {
                Ok(meta) => {
                    let ft = meta.file_type();
                    if ft.is_file() || ft.is_symlink() {
                        std::fs::remove_file(&self.dst)?;
                    } else if ft.is_dir() {
                        std::fs::remove_dir_all(&self.dst)?;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    // 目标不存在，无需清理
                }
                Err(e) => return Err(e.into()),
            }
        }
        self.mode.create(self)?;
        Ok(())
    }

    pub fn remove(&self) -> Result<()> {
        self.mode.remove(self)?;
        Ok(())
    }
}

impl SymlinkMode {
    fn create(&self, symlink: &Symlink) -> Result<()> {
        match self {
            SymlinkMode::Symlink => {
                std::os::unix::fs::symlink(&symlink.src, &symlink.dst)
                    .with_context(|| format!("failed to create symlink: {symlink}"))?;
                Ok(())
            }
            SymlinkMode::Copy => {
                std::fs::copy(&symlink.src, &symlink.dst)
                    .with_context(|| format!("failed to create symlink: {symlink}"))?;
                Ok(())
            }
            SymlinkMode::Move => {
                // 优先尝试 rename（同文件系统下高效），失败则回退到 copy+delete
                if std::fs::rename(&symlink.src, &symlink.dst).is_err() {
                    let meta = std::fs::symlink_metadata(&symlink.src)
                        .with_context(|| format!("failed to read metadata: {symlink}"))?;
                    if meta.is_dir() {
                        copy_dir_all(&symlink.src, &symlink.dst)
                            .with_context(|| format!("failed to copy dir: {symlink}"))?;
                        std::fs::remove_dir_all(&symlink.src)
                            .with_context(|| format!("failed to remove source dir: {symlink}"))?;
                    } else {
                        std::fs::copy(&symlink.src, &symlink.dst)
                            .with_context(|| format!("failed to copy file: {symlink}"))?;
                        std::fs::remove_file(&symlink.src)
                            .with_context(|| format!("failed to remove source file: {symlink}"))?;
                    }
                }
                Ok(())
            }
        }
    }

    fn remove(&self, symlink: &Symlink) -> Result<()> {
        match self {
            SymlinkMode::Symlink => {
                // 用 symlink_metadata 一次性获取元数据，避免多次 stat() 调用之间的 TOCTOU 竞态窗口
                match std::fs::symlink_metadata(&symlink.dst) {
                    Ok(meta) => {
                        if meta.file_type().is_symlink() {
                            std::fs::remove_file(&symlink.dst)
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
            SymlinkMode::Copy | SymlinkMode::Move => {
                std::fs::remove_file(&symlink.dst)
                    .with_context(|| format!("failed to remove symlink: {symlink}"))?;
                Ok(())
            }
        }
    }
}

/// 递归复制目录（用于 Move 模式的跨文件系统回退）
fn copy_dir_all(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_child = entry.path();
        let dst_child = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&src_child, &dst_child)?;
        } else {
            std::fs::copy(&src_child, &dst_child)?;
        }
    }
    Ok(())
}
