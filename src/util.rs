use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::error::{anyhow, Result};

pub(crate) fn shell_expend_full<P: AsRef<Path>>(path: P) -> Result<PathBuf> {
    let origin = path
        .as_ref()
        .to_str()
        .ok_or_else(|| anyhow!("path error"))?;
    return Ok(PathBuf::from(
        shellexpand::tilde(shellexpand::full(origin)?.as_ref()).as_ref(),
    ));
}

pub(crate) fn expand_symlink_dir(expand_symlink: impl AsRef<Path>) -> Result<()> {
    let sub_paths = std::fs::read_dir(&expand_symlink)?;
    let point_to = std::fs::read_link(&expand_symlink)?;
    std::fs::remove_file(&expand_symlink)?;
    std::fs::create_dir_all(&expand_symlink)?;
    for sub_path in sub_paths {
        let sub_path = sub_path?;
        std::os::unix::fs::symlink(
            point_to.join(sub_path.path().strip_prefix(&expand_symlink)?),
            sub_path.path(),
        )?;
    }
    Ok(())
}

pub(crate) fn is_empty_dir(path: impl AsRef<Path>) -> bool {
    !path.as_ref().exists()
        || (path.as_ref().is_dir()
            && walkdir::WalkDir::new(path)
                .follow_links(true)
                .into_iter()
                .filter_entry(|e| e.file_type().is_file())
                .next()
                .is_none())
}

pub(crate) fn find_symlink_at(
    dir_path: impl AsRef<Path>,
    link_prefix: impl AsRef<Path>,
) -> Result<Vec<(PathBuf, PathBuf)>> {
    let mut paths = Vec::new();
    if dir_path.as_ref().exists() {
        for entry in WalkDir::new(dir_path).follow_links(false) {
            let entry = entry?;
            let path = entry.into_path();
            if path.is_symlink() {
                let point_to = std::fs::read_link(&path)?;
                if point_to.starts_with(&link_prefix) {
                    paths.push((path, point_to));
                }
            }
        }
    }
    Ok(paths)
}
