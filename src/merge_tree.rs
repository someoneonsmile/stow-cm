use regex::RegexSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::Result;
use crate::merge::Merge;
use crate::symlink::{Symlink, SymlinkMode};
use crate::util;

#[derive(Debug)]
pub(crate) struct MergeTree {
    target: PathBuf,
    source: PathBuf,
    option: Option<Arc<MergeOption>>,
}

#[derive(Debug)]
pub(crate) struct MergeOption {
    pub ignore: Option<RegexSet>,
    pub over: Option<RegexSet>,
    pub fold: Option<bool>,
    pub symlink_mode: Option<SymlinkMode>,
}

#[derive(Debug)]
pub(crate) struct MergeResult {
    /// conflict file or dir
    pub conflicts: Option<Vec<PathBuf>>,
    /// install paths
    pub to_create_symlinks: Option<Vec<Symlink>>,
    /// expand the symlink dir
    pub expand_symlinks: Option<Vec<PathBuf>>,
    /// is there has ignore file under the dir
    pub has_ignore: bool,
    /// can fold
    pub foldable: bool,
}

impl MergeTree {
    pub(crate) fn new(
        target: impl AsRef<Path>,
        source: impl AsRef<Path>,
        option: Option<Arc<MergeOption>>,
    ) -> Self {
        MergeTree {
            target: target.as_ref().to_path_buf(),
            source: source.as_ref().to_path_buf(),
            option,
        }
    }

    /// 从树的叶子节点回溯
    /// 没有 Ignore 的时候, 折叠目录
    /// 返回当前根节点
    pub(crate) fn merge_add(self) -> Result<MergeResult> {
        // source not exists
        if !self.source.exists() {
            return Ok(MergeResult {
                conflicts: None,
                to_create_symlinks: None,
                expand_symlinks: None,
                has_ignore: false,
                foldable: true,
            });
        }

        // source ignore
        if let Some(ignore_re) = self.option.as_ref().and_then(|it| it.ignore.as_ref()) {
            if ignore_re.is_match(&self.source.to_string_lossy()) {
                return Ok(MergeResult {
                    conflicts: None,
                    expand_symlinks: None,
                    to_create_symlinks: None,
                    has_ignore: true,
                    foldable: false,
                });
            }
        }

        // same file
        if self.target.exists() && same_file::is_same_file(&self.target, &self.source)? {
            return Ok(MergeResult {
                conflicts: None,
                expand_symlinks: None,
                to_create_symlinks: Some(vec![Symlink {
                    src: self.source,
                    dst: self.target,
                    mode: self
                        .option
                        .as_ref()
                        .and_then(|it| it.symlink_mode.clone())
                        .unwrap_or_default(),
                }]),
                has_ignore: false,
                foldable: true,
            });
        }

        // conflict check
        if check_conflict(
            &self.source,
            &self.target,
            self.option.as_ref().and_then(|it| it.over.as_ref()),
        ) {
            return Ok(MergeResult {
                conflicts: Some(vec![self.source]),
                expand_symlinks: None,
                to_create_symlinks: None,
                has_ignore: false,
                foldable: false,
            });
        }

        // source is file
        if self.source.is_file() {
            return Ok(MergeResult {
                conflicts: None,
                expand_symlinks: None,
                to_create_symlinks: Some(vec![Symlink {
                    src: self.source,
                    dst: self.target,
                    mode: self
                        .option
                        .as_ref()
                        .and_then(|it| it.symlink_mode.clone())
                        .unwrap_or_default(),
                }]),
                has_ignore: false,
                foldable: true,
            });
        }

        // source is dir
        let mut has_ignore = false;
        let mut conflicts = None;
        let mut install_paths = None;
        let mut expand_symlinks = None;
        let mut foldable = true;

        // expand symlink (/symlink/subpath is symlink too?)
        if self.target.is_symlink() {
            expand_symlinks = Some(vec![self.target.clone()]);
        }

        for path in fs::read_dir(&self.source)? {
            let path = &path?.path();
            let sub_result = MergeTree::new(
                self.target.join(path.strip_prefix(&self.source)?),
                path,
                self.option.clone(),
            )
            .merge_add()?;
            has_ignore |= sub_result.has_ignore;
            conflicts = conflicts.merge(sub_result.conflicts);
            expand_symlinks = expand_symlinks.merge(sub_result.expand_symlinks);
            install_paths = install_paths.merge(sub_result.to_create_symlinks);
            foldable &= sub_result.foldable;
        }
        // is there has other tree file?
        foldable = foldable && !util::has_new_sub(&self.target, &self.source)?;

        // fold dir
        if let Some(true) = self.option.as_ref().and_then(|it| it.fold) {
            if foldable {
                return Ok(MergeResult {
                    conflicts: None,
                    expand_symlinks: None,
                    to_create_symlinks: Some(vec![Symlink {
                        src: self.source,
                        dst: self.target,
                        mode: self
                            .option
                            .as_ref()
                            .and_then(|it| it.symlink_mode.clone())
                            .unwrap_or_default(),
                    }]),
                    has_ignore: false,
                    foldable: true,
                });
            }
        }

        Ok(MergeResult {
            conflicts,
            expand_symlinks,
            to_create_symlinks: install_paths,
            has_ignore,
            foldable,
        })
    }
}

fn check_conflict(
    source: impl AsRef<Path>,
    target: impl AsRef<Path>,
    over: Option<&RegexSet>,
) -> bool {
    let source = source.as_ref();
    let target = target.as_ref();
    if !target.exists() {
        return false;
    }
    if let Some(over_re) = over {
        // over
        if over_re.is_match(&source.to_string_lossy()) {
            return false;
        }
    }
    target.is_file() || source.is_file()
}

#[cfg(test)]
mod test {
    use anyhow::Result;
    use std::env::temp_dir;

    #[test]
    fn is_symlink() -> Result<()> {
        std::fs::create_dir_all(temp_dir().join("path"))?;
        std::os::unix::fs::symlink(temp_dir().join("path"), temp_dir().join("link"))?;
        std::os::unix::fs::symlink(
            temp_dir().join("subfile"),
            temp_dir().join("path").join("sublink"),
        )?;
        std::fs::create_dir_all(temp_dir().join("path").join("subpath"))?;
        std::fs::File::create(temp_dir().join("path").join("subfile"))?;

        assert!(temp_dir().join("link").is_symlink());
        assert!(temp_dir().join("link").join("sublink").is_symlink());
        assert!(!temp_dir().join("link").join("subpath").is_symlink());
        assert!(!temp_dir().join("link").join("subfile").is_symlink());
        // is file
        assert!(temp_dir().join("link").join("subfile").is_file());
        assert!(!temp_dir().join("link").join("sublink").is_file());

        // clean up
        std::fs::remove_dir_all(temp_dir().join("path"))?;
        std::fs::remove_file(temp_dir().join("link"))?;
        Ok(())
    }

    #[test]
    fn re_match() -> Result<()> {
        assert!(regex::RegexSet::new(vec![".*"])?.is_match("/path/somepath/somefile.suffix"));
        Ok(())
    }
}
