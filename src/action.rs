use std::fmt;
use std::path::PathBuf;

use crate::symlink::SymlinkMode;

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
    DecryptFile { src: PathBuf, to: PathBuf },
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
}

/// 动作计划，包含一组待执行的操作及统计信息
#[derive(Debug, Clone)]
pub struct ActionPlan {
    pub actions: Vec<Action>,
    pub stats: PlanStats,
}

impl ActionPlan {
    /// 创建一个空的行动计划
    pub fn new() -> Self {
        Self {
            actions: Vec::new(),
            stats: PlanStats::default(),
        }
    }

    /// 判断计划是否为空
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }

    /// 判断计划中是否存在冲突
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
            Action::DecryptFile { src, to } => {
                write!(f, "~ DECRYPT  {}  ->  {}", src.display(), to.display())
            }
        }
    }
}

impl fmt::Display for ActionPlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            return writeln!(f, "── Plan (empty) ──");
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
        if stats.conflicts > 0 {
            parts.push(format!("{}!", stats.conflicts));
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
            Action::Conflict {
                dst: PathBuf::from("/dst/c"),
                reason: "exists".to_string(),
            },
        ];
        let plan = ActionPlan {
            actions,
            stats: PlanStats {
                links_to_create: 1,
                links_to_remove: 1,
                dirs_to_create: 0,
                conflicts: 1,
                ignored: 0,
                overridden: 2,
                encrypted: 3,
            },
        };
        let output = format!("{plan}");
        assert!(output.contains("── Plan ──"), "output: {output}");
        assert!(output.contains("+ CREATE"), "output: {output}");
        assert!(output.contains("- REMOVE"), "output: {output}");
        assert!(output.contains("! CONFLICT"), "output: {output}");
        // 页脚：1c, 1r, 1!, 2o, 3e
        assert!(output.contains("1c"), "output: {output}");
        assert!(output.contains("1r"), "output: {output}");
        assert!(output.contains("1!"), "output: {output}");
        assert!(output.contains("2o"), "output: {output}");
        assert!(output.contains("3e"), "output: {output}");
        // ignored 为 0，不应出现在页脚
        assert!(!output.contains("0i"), "output: {output}");
    }
}
