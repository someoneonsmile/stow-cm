use regex::RegexSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{anyhow, Result};

pub struct CollectBot<'a> {
    path: PathBuf,
    ignore: &'a Option<RegexSet>,
}

impl<'a> CollectBot<'a> {
    pub fn new<P: AsRef<Path>>(path: P, ignore: &'a Option<RegexSet>) -> Self {
        CollectBot {
            path: path.as_ref().to_path_buf(),
            ignore,
        }
    }

    /// 从树的叶子节点回溯
    /// 没有 Ignore 的时候, 折叠目录
    /// 返回当前根节点
    pub fn collect(self) -> Result<(bool, Option<Vec<PathBuf>>)> {
        if !self.path.exists() {
            return Ok((false, None));
        }

        if let Some(ignore_re) = &self.ignore {
            if ignore_re.is_match(
                self.path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .ok_or(anyhow!("invalid file name"))?,
            ) {
                return Ok((true, None));
            }
        }

        if self.path.is_file() {
            return Ok((false, Some(vec![self.path])));
        }

        let mut has_ignore = false;

        let mut paths = Vec::new();
        for path in fs::read_dir(&self.path)? {
            let (sub_ignore, sub_paths_option) =
                CollectBot::new(&path?.path(), &self.ignore).collect()?;
            has_ignore |= sub_ignore;
            if let Some(mut sub_paths) = sub_paths_option {
                paths.append(&mut sub_paths)
            }
        }

        if !has_ignore {
            return Ok((false, Some(vec![self.path])));
        }

        return Ok((true, Some(paths)));
    }
}
