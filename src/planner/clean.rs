//! 清理计划生成子模块。
//!
//! 包含 [`plan_clean`]。

use std::path::Path;

use crate::action::{Action, ActionPlan, PlanStats};
use crate::symlink::Symlink;

/// 生成清理计划。
///
/// 将文件系统扫描得到的符号链接列表转换为 `ActionPlan`，
/// 每条链接生成一个 `RemoveLink` 操作。
/// 若指定了 `decrypted_path` 则追加 `RemoveDir` 操作，
/// 若指定了 `state_dir` 则追加 `RemoveDir` 清理 pack state 目录。
#[must_use]
pub fn plan_clean(
    symlinks: &[Symlink],
    decrypted_path: Option<&Path>,
    state_dir: Option<&Path>,
) -> ActionPlan {
    let actions: Vec<Action> = symlinks
        .iter()
        .map(|s| Action::RemoveLink {
            src: s.src.clone(),
            dst: s.dst.clone(),
            mode: s.mode.clone(),
        })
        .collect();

    let mut plan = ActionPlan {
        stats: PlanStats {
            links_to_remove: actions.len(),
            ..PlanStats::default()
        },
        actions,
    };

    if let Some(path) = decrypted_path {
        plan.actions.push(Action::RemoveDir {
            path: path.to_path_buf(),
            reason: "cleanup decrypted files directory".to_string(),
        });
        plan.stats.dirs_removed += 1;
    }

    if let Some(dir) = state_dir {
        plan.actions.push(Action::RemoveDir {
            path: dir.to_path_buf(),
            reason: "cleanup pack state directory".to_string(),
        });
        plan.stats.dirs_removed += 1;
    }

    plan
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::action::Action;
    use crate::symlink::{Symlink, SymlinkMode};

    #[test]
    fn plan_clean_basic() {
        let symlinks = [Symlink {
            src: PathBuf::from("/pack/file.txt"),
            dst: PathBuf::from("/target/file.txt"),
            mode: SymlinkMode::Symlink,
        }];

        let plan = plan_clean(&symlinks, None, None);

        assert_eq!(plan.stats.links_to_remove, 1);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(plan.actions[0], Action::RemoveLink { .. }));
    }

    #[test]
    fn plan_clean_empty() {
        let symlinks: [Symlink; 0] = [];

        let plan = plan_clean(&symlinks, None, None);

        assert!(plan.is_empty());
        assert_eq!(plan.stats.links_to_remove, 0);
    }

    #[test]
    fn plan_clean_multiple_links() {
        let symlinks = [
            Symlink {
                src: PathBuf::from("/pack/a.txt"),
                dst: PathBuf::from("/target/a.txt"),
                mode: SymlinkMode::Symlink,
            },
            Symlink {
                src: PathBuf::from("/pack/b.txt"),
                dst: PathBuf::from("/target/b.txt"),
                mode: SymlinkMode::Copy,
            },
        ];

        let plan = plan_clean(&symlinks, None, None);

        assert_eq!(plan.stats.links_to_remove, 2);
        assert_eq!(plan.actions.len(), 2);
    }
}
