use std::fmt;
use std::path::PathBuf;

use crate::symlink::SymlinkMode;
use crate::track_file::Track;

/// [`Action::RemoveDir`] 的删除语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoveDirMode {
    /// 递归删除目录及其全部内容：state 目录、解密目录的清理，
    /// 以及折叠/覆盖时对旧目录的替换。
    All,
    /// 仅当目录子树中不含任何文件/链接时删除；含文件/链接则报错，绝不误删。
    ///
    /// 用于「移除链接后清理空目录」这类基于虚拟树推断出的空目录：
    /// 计划层只列出最顶层空目录，执行时允许递归清理嵌套空目录，
    /// 但一旦发现文件或链接就终止并报错，暴露「计划与磁盘不一致」。
    IfEmpty,
}

/// 动作枚举，表示计划中的一个操作步骤
#[derive(Debug, Clone)]
pub enum Action {
    /// 创建链接（软链接或复制）
    CreateLink {
        src: PathBuf,
        dst: PathBuf,
        mode: SymlinkMode,
    },
    /// 移除链接
    RemoveLink {
        src: PathBuf,
        dst: PathBuf,
        mode: SymlinkMode,
    },
    /// 创建目录
    CreateDir(PathBuf),
    /// 冲突（目标已存在且非链接文件）
    Conflict { dst: PathBuf, reason: String },
    /// 解密文件
    DecryptFile {
        src: PathBuf,
        to: PathBuf,
        key: Vec<u8>,
        alg: String,
        left_boundary: String,
        right_boundary: String,
    },
    /// 移除目录
    RemoveDir {
        path: PathBuf,
        reason: String,
        mode: RemoveDirMode,
    },
    /// 移除文件
    RemoveFile { path: PathBuf, reason: String },
    /// 写入 track file
    WriteTrackFile { path: PathBuf, track: Track },
}

/// 计划统计信息，记录各类操作的数量
#[derive(Debug, Clone, Default)]
pub struct PlanStats {
    pub links_to_create: usize,
    pub links_to_remove: usize,
    pub dirs_to_create: usize,
    pub conflicts: usize,
    pub ignored: usize,
    pub overridden: usize,
    pub encrypted: usize,
    pub dirs_removed: usize,
    /// 移除文件的数量
    pub files_removed: usize,
}

/// 动作计划，包含一组待执行的操作及统计信息
#[derive(Debug, Clone)]
pub struct ActionPlan {
    pub actions: Vec<Action>,
    pub stats: PlanStats,
}

impl Default for ActionPlan {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionPlan {
    /// 创建一个空的行动计划
    #[must_use]
    pub fn new() -> Self {
        Self {
            actions: Vec::new(),
            stats: PlanStats::default(),
        }
    }

    /// 判断计划是否为空
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// 判断计划中是否存在冲突
    #[must_use]
    pub fn has_conflicts(&self) -> bool {
        self.stats.conflicts > 0
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::CreateLink { src, dst, mode } => {
                write!(
                    f,
                    "+ CREATE   {}  ->  {}  [{}]",
                    dst.display(),
                    src.display(),
                    format!("{mode:?}").to_lowercase()
                )
            }
            Action::RemoveLink { src, dst, mode } => {
                write!(
                    f,
                    "- REMOVE   {}  ->  {}  [{}]",
                    dst.display(),
                    src.display(),
                    format!("{mode:?}").to_lowercase()
                )
            }
            Action::CreateDir(path) => {
                write!(f, "# MKDIR    {}", path.display())
            }
            Action::Conflict { dst, reason } => {
                write!(f, "! CONFLICT {}  ({})", dst.display(), reason)
            }
            Action::DecryptFile { src, to, .. } => {
                write!(f, "~ DECRYPT  {}  ->  {}", src.display(), to.display())
            }
            Action::RemoveDir { path, reason, .. } => {
                write!(f, "- RMDIR    {}  ({})", path.display(), reason)
            }
            Action::RemoveFile { path, reason } => {
                write!(f, "- RM       {}  ({})", path.display(), reason)
            }
            Action::WriteTrackFile { path, .. } => {
                write!(f, "≈ TRACK   {}", path.display())
            }
        }
    }
}

impl fmt::Display for ActionPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return writeln!(f, "── Plan (empty) ──");
        }

        // 存在冲突时只展示冲突，避免与其他操作（如 RMDIR）混在一起造成困惑
        if self.has_conflicts() {
            writeln!(f, "── Conflicts ({}) ──", self.stats.conflicts)?;
            for action in &self.actions {
                if let Action::Conflict { .. } = action {
                    writeln!(f, "{action}")?;
                }
            }
            return Ok(());
        }

        writeln!(f, "── Plan ──")?;
        for action in &self.actions {
            writeln!(f, "{action}")?;
        }

        // 构建页脚，仅包含非零统计项
        let mut parts: Vec<String> = Vec::new();
        let stats = &self.stats;
        if stats.links_to_create > 0 {
            parts.push(format!("{}c", stats.links_to_create));
        }
        if stats.links_to_remove > 0 {
            parts.push(format!("{}r", stats.links_to_remove));
        }
        if stats.ignored > 0 {
            parts.push(format!("{}i", stats.ignored));
        }
        if stats.overridden > 0 {
            parts.push(format!("{}o", stats.overridden));
        }
        if stats.encrypted > 0 {
            parts.push(format!("{}e", stats.encrypted));
        }
        if stats.dirs_removed > 0 {
            parts.push(format!("{}rd", stats.dirs_removed));
        }
        if stats.files_removed > 0 {
            parts.push(format!("{}rf", stats.files_removed));
        }

        if parts.is_empty() {
            write!(f, "── ──")?;
        } else {
            write!(f, "── {} ──", parts.join(", "))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_plan_display() {
        let plan = ActionPlan::new();
        let output = format!("{plan}");
        assert!(
            output.contains("(empty)"),
            "expected '(empty)' in: {output}"
        );
    }

    #[test]
    fn test_create_link_display() {
        let action = Action::CreateLink {
            src: PathBuf::from("/src/file"),
            dst: PathBuf::from("/dst/file"),
            mode: SymlinkMode::Symlink,
        };
        let output = format!("{action}");
        assert!(output.contains("+ CREATE"), "output: {output}");
        assert!(output.contains("/dst/file"), "output: {output}");
        assert!(output.contains("/src/file"), "output: {output}");
        assert!(output.contains("[symlink]"), "output: {output}");
    }

    #[test]
    fn test_remove_link_display() {
        let action = Action::RemoveLink {
            src: PathBuf::from("/src/file"),
            dst: PathBuf::from("/dst/file"),
            mode: SymlinkMode::Copy,
        };
        let output = format!("{action}");
        assert!(output.contains("- REMOVE"), "output: {output}");
        assert!(output.contains("/dst/file"), "output: {output}");
        assert!(output.contains("[copy]"), "output: {output}");
        assert!(output.contains("/src/file"), "output: {output}");
    }

    #[test]
    fn test_conflict_display() {
        let action = Action::Conflict {
            dst: PathBuf::from("/dst/file"),
            reason: "file already exists".to_string(),
        };
        let output = format!("{action}");
        assert!(output.contains("! CONFLICT"), "output: {output}");
        assert!(output.contains("/dst/file"), "output: {output}");
        assert!(output.contains("file already exists"), "output: {output}");
    }

    #[test]
    fn test_remove_dir_display() {
        let action = Action::RemoveDir {
            path: PathBuf::from("/tmp/decrypted"),
            reason: "cleanup decrypted files".to_string(),
            mode: RemoveDirMode::All,
        };
        let output = format!("{action}");
        assert!(output.contains("- RMDIR"), "output: {output}");
        assert!(output.contains("/tmp/decrypted"), "output: {output}");
        assert!(
            output.contains("cleanup decrypted files"),
            "output: {output}"
        );
    }

    #[test]
    fn test_remove_file_display() {
        let action = Action::RemoveFile {
            path: PathBuf::from("/tmp/stale.txt"),
            reason: "orphaned track file".to_string(),
        };
        let output = format!("{action}");
        assert!(output.contains("- RM"), "output: {output}");
        assert!(output.contains("/tmp/stale.txt"), "output: {output}");
        assert!(output.contains("orphaned track file"), "output: {output}");
    }

    #[test]
    fn test_plan_display_multiple() {
        let actions = vec![
            Action::CreateLink {
                src: PathBuf::from("/src/a"),
                dst: PathBuf::from("/dst/a"),
                mode: SymlinkMode::Symlink,
            },
            Action::RemoveLink {
                src: PathBuf::from("/src/b"),
                dst: PathBuf::from("/dst/b"),
                mode: SymlinkMode::Copy,
            },
        ];
        let plan = ActionPlan {
            actions,
            stats: PlanStats {
                links_to_create: 1,
                links_to_remove: 1,
                dirs_to_create: 0,
                conflicts: 0,
                ignored: 0,
                overridden: 2,
                encrypted: 3,
                dirs_removed: 0,
                files_removed: 1,
            },
        };
        let output = format!("{plan}");
        assert!(output.contains("── Plan ──"), "output: {output}");
        assert!(output.contains("+ CREATE"), "output: {output}");
        assert!(output.contains("- REMOVE"), "output: {output}");
        // 页脚：1c, 1r, 2o, 3e, 1rf
        assert!(output.contains("1c"), "output: {output}");
        assert!(output.contains("1r"), "output: {output}");
        assert!(output.contains("2o"), "output: {output}");
        assert!(output.contains("3e"), "output: {output}");
        assert!(output.contains("1rf"), "output: {output}");
        // ignored、conflicts 为 0，不应出现在页脚
        assert!(!output.contains("0i"), "output: {output}");
        assert!(!output.contains("0!"), "output: {output}");
    }

    #[test]
    fn test_plan_display_only_conflicts() {
        let actions = vec![
            Action::Conflict {
                dst: PathBuf::from("/dst/a"),
                reason: "file already exists".to_string(),
            },
            Action::Conflict {
                dst: PathBuf::from("/dst/b"),
                reason: "expected symlink, found directory".to_string(),
            },
            Action::RemoveLink {
                src: PathBuf::from("/src/c"),
                dst: PathBuf::from("/dst/c"),
                mode: SymlinkMode::Symlink,
            },
        ];
        let plan = ActionPlan {
            actions,
            stats: PlanStats {
                links_to_remove: 1,
                conflicts: 2,
                ..PlanStats::default()
            },
        };
        let output = format!("{plan}");
        // 有冲突时只显示冲突，不显示其他操作也不需要页脚
        assert!(output.contains("── Conflicts (2) ──"), "output: {output}");
        assert!(output.contains("! CONFLICT"), "output: {output}");
        assert!(!output.contains("── Plan ──"), "output: {output}");
        assert!(!output.contains("- REMOVE"), "output: {output}");
        assert!(!output.contains("1r"), "output: {output}");
    }
}
