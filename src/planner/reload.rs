//! 重载计划与计划合并子模块。
//!
//! 包含 [`plan_reload`]、[`merge_and_dedup`] 和 [`combine_plans`]。

use std::path::PathBuf;

use crate::action::{Action, ActionPlan, PlanStats};
use crate::error::Result;
use crate::symlink::SymlinkMode;
use crate::track_file::Track;

use super::{PlanOption, VNode};

/// 为 reload 操作生成合并后的移除+安装计划。
///
/// # 去重逻辑
///
/// Symlink→Symlink reload（track 原本是 symlink，配置也是 symlink）：
///
/// - 若同一 `dst` 在移除和安装计划中同时出现，且 `src` 相同 → 同时删除此二操作（无净变更）
/// - 若 `src` 不同 → 两者都保留
/// - 去重仅影响 `RemoveLink`/`CreateLink` 对；解密目录的 `RemoveDir`/`CreateDir` 不受影响
///
/// Copy/Move 模式的 reload：
/// copy 模式不做去重，remove 和 install 各自独立执行。
pub fn plan_reload(
    pack_tree: &mut VNode,
    remove_target: &mut VNode,
    install_target: Option<&mut VNode>,
    track: &Track,
    options: &PlanOption,
) -> Result<ActionPlan> {
    let remove_plan = super::remove::plan_remove(track, remove_target, None);

    // 移除阶段有冲突时跳过安装，executor 会因冲突终止整个 reload
    if remove_plan.has_conflicts() {
        return Ok(remove_plan);
    }

    // install_target 为 None 时表示与 remove 同一棵树 → 使用清理后的版本，
    // 但需先将 remove 阶段留下的 ShallowDir 按 pack 树展开为完整 Dir 节点
    let install_plan = if let Some(install_tree) = install_target {
        super::install::plan_install(pack_tree, install_tree, options)?
    } else {
        remove_target.expand_shallow(pack_tree)?;
        super::install::plan_install(pack_tree, remove_target, options)?
    };

    let track_mode = track.symlink_mode.as_ref().unwrap_or(&SymlinkMode::Symlink);
    let config_mode = options
        .merge
        .symlink_mode
        .as_ref()
        .unwrap_or(&SymlinkMode::Symlink);
    // 仅 Symlink→Symlink 时做 link 去重（加密场景同样生效：
    // RemoveLink+CreateLink 相同 src/dst 即抵消，但解密目录的 RemoveDir+CreateDir 不受影响）
    let should_dedup = *track_mode == SymlinkMode::Symlink && *config_mode == SymlinkMode::Symlink;

    let merged = if should_dedup {
        merge_and_dedup(remove_plan, install_plan)
    } else {
        combine_plans(remove_plan, install_plan)
    };

    Ok(merged)
}

/// 合并移除计划和安装计划，按 `dst` 去重。
///
/// 若同一 `dst` 路径同时出现在移除和安装操作中，且两者 `src` 相同，
/// 则两操作相互抵消（无净变更），从合并计划中删除。
fn merge_and_dedup(remove_plan: ActionPlan, mut install_plan: ActionPlan) -> ActionPlan {
    // 构建 install_plan 中 (dst, src) 的索引以便去重
    use std::collections::HashMap;

    let install_index: HashMap<PathBuf, PathBuf> = install_plan
        .actions
        .iter()
        .filter_map(|a| match a {
            Action::CreateLink { dst, src, .. } => Some((dst.clone(), src.clone())),
            _ => None,
        })
        .collect();

    // 收集需要从 remove_plan 中移除的 (dst, src) 对
    let to_remove_from_install: Vec<(PathBuf, PathBuf)> = remove_plan
        .actions
        .iter()
        .filter_map(|a| {
            if let Action::RemoveLink { dst, src, .. } = a
                && let Some(install_src) = install_index.get(dst)
                && install_src == src
            {
                return Some((dst.clone(), src.clone()));
            }
            None
        })
        .collect();

    // 从 install_plan 中移除匹配的 CreateLink
    install_plan.actions.retain(|a| {
        if let Action::CreateLink { dst, src, .. } = a
            && to_remove_from_install
                .iter()
                .any(|(d, s)| d == dst && s == src)
        {
            install_plan.stats.links_to_create =
                install_plan.stats.links_to_create.saturating_sub(1);
            return false;
        }
        true
    });

    // 从 remove_plan 中移除匹配的 RemoveLink
    let mut filtered_remove_actions: Vec<Action> = Vec::new();
    let mut remove_count = remove_plan.stats.links_to_remove;

    for a in remove_plan.actions {
        if let Action::RemoveLink {
            ref dst, ref src, ..
        } = a
            && to_remove_from_install
                .iter()
                .any(|(d, s)| d == dst && s == src)
        {
            remove_count = remove_count.saturating_sub(1);
            continue;
        }
        filtered_remove_actions.push(a);
    }

    // 合并统计
    let combined_stats = PlanStats {
        links_to_create: install_plan.stats.links_to_create,
        links_to_remove: remove_count,
        dirs_to_create: install_plan.stats.dirs_to_create + remove_plan.stats.dirs_to_create,
        conflicts: install_plan.stats.conflicts + remove_plan.stats.conflicts,
        ignored: install_plan.stats.ignored + remove_plan.stats.ignored,
        overridden: install_plan.stats.overridden + remove_plan.stats.overridden,
        encrypted: install_plan.stats.encrypted + remove_plan.stats.encrypted,
        dirs_removed: install_plan.stats.dirs_removed + remove_plan.stats.dirs_removed,
        files_removed: install_plan.stats.files_removed + remove_plan.stats.files_removed,
    };

    // 合并操作列表：remove 在前，install 在后
    let mut merged_actions = filtered_remove_actions;
    merged_actions.extend(install_plan.actions);

    ActionPlan {
        actions: merged_actions,
        stats: combined_stats,
    }
}

/// 简单拼接 remove 和 install 计划（不做去重）。
/// remove 的操作在前，install 的操作在后。
fn combine_plans(remove_plan: ActionPlan, install_plan: ActionPlan) -> ActionPlan {
    let mut merged_actions = remove_plan.actions;
    let combined_stats = PlanStats {
        links_to_create: install_plan.stats.links_to_create,
        links_to_remove: remove_plan.stats.links_to_remove,
        dirs_to_create: install_plan.stats.dirs_to_create + remove_plan.stats.dirs_to_create,
        conflicts: install_plan.stats.conflicts + remove_plan.stats.conflicts,
        ignored: install_plan.stats.ignored + remove_plan.stats.ignored,
        overridden: install_plan.stats.overridden + remove_plan.stats.overridden,
        encrypted: install_plan.stats.encrypted + remove_plan.stats.encrypted,
        dirs_removed: install_plan.stats.dirs_removed + remove_plan.stats.dirs_removed,
        files_removed: install_plan.stats.files_removed + remove_plan.stats.files_removed,
    };
    merged_actions.extend(install_plan.actions);
    ActionPlan {
        actions: merged_actions,
        stats: combined_stats,
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use std::path::PathBuf;

    use super::super::{default_options, dir_node, file_node, make_track, symlink_node};
    use super::*;
    use crate::action::Action;
    use crate::symlink::SymlinkMode;

    #[test]
    fn plan_reload_dedup_same_src() {
        // 同一 dst 在 remove 和 install 中 src 相同 → 抵消
        let mut pack = dir_node("", "/pack", vec![file_node("file.txt", "/pack/file.txt")]);
        // target_tree 包含目标文件（Symlink 节点指向正确的 src），以便 plan_remove 能找到并移除它
        let mut target = dir_node(
            "",
            "/target",
            vec![symlink_node(
                "file.txt",
                "/target/file.txt",
                "/pack/file.txt",
            )],
        );
        let track = make_track("/pack/file.txt", "/target/file.txt");

        let plan = plan_reload(&mut pack, &mut target, None, &track, &default_options()).unwrap();

        // 同一路径同时移除和创建，src 相同，互相抵消
        assert_eq!(plan.stats.links_to_create, 0);
        assert_eq!(plan.stats.links_to_remove, 0);
        assert_eq!(plan.actions.len(), 0);
    }

    #[test]
    fn plan_reload_different_src() {
        // 同一 dst，但 src 不同（文件位置变更）→ 两者都保留
        let mut pack = dir_node(
            "",
            "/pack",
            vec![file_node("file.txt", "/pack/new/file.txt")],
        );
        // target_tree 包含指向旧 src 的 symlink，与 track 一致，plan_remove 生成 RemoveLink
        // 注意：这里 src="/pack/old/file.txt" 与 track 一致，但 install plan 的 src 是 "/pack/new/file.txt"
        let mut target = dir_node(
            "",
            "/target",
            vec![symlink_node(
                "file.txt",
                "/target/file.txt",
                "/pack/old/file.txt",
            )],
        );
        let track = make_track("/pack/old/file.txt", "/target/file.txt");

        let plan = plan_reload(&mut pack, &mut target, None, &track, &default_options()).unwrap();

        // src 不同 → remove 和 create 都应保留
        assert_eq!(plan.stats.links_to_create, 1);
        assert_eq!(plan.stats.links_to_remove, 1);
        assert_eq!(plan.actions.len(), 2);
    }

    #[test]
    fn plan_reload_new_file() {
        // pack 中有新文件，track 为空
        let mut pack = dir_node(
            "",
            "/pack",
            vec![file_node("new_file.txt", "/pack/new_file.txt")],
        );
        let mut target = dir_node("", "/target", Vec::new());
        let track = Track {
            links: Vec::new(),
            decrypted_path: None,
            encrypted: false,
            pack_name: None,
            pack_path: None,
            target: None,
            symlink_mode: None,
        };

        let plan = plan_reload(&mut pack, &mut target, None, &track, &default_options()).unwrap();

        assert_eq!(plan.stats.links_to_create, 1);
        assert_eq!(plan.stats.links_to_remove, 0);
        assert_eq!(plan.actions.len(), 1);
    }

    #[test]
    fn plan_reload_removed_file() {
        // pack 中删除了文件（track 有记录但 pack 没有）
        let mut pack = dir_node("", "/pack", Vec::new());
        let mut target = dir_node(
            "",
            "/target",
            vec![symlink_node("old.txt", "/target/old.txt", "/pack/old.txt")],
        );
        let track = make_track("/pack/old.txt", "/target/old.txt");

        let plan = plan_reload(&mut pack, &mut target, None, &track, &default_options()).unwrap();

        // 只有 remove，没有 create
        assert_eq!(plan.stats.links_to_create, 0);
        assert_eq!(plan.stats.links_to_remove, 1);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(plan.actions[0], Action::RemoveLink { .. }));
    }

    #[test]
    fn merge_and_dedup_removes_matching_pair() {
        let remove_plan = ActionPlan {
            actions: vec![Action::RemoveLink {
                src: PathBuf::from("/pack/file.txt"),
                dst: PathBuf::from("/target/file.txt"),
                mode: SymlinkMode::Symlink,
            }],
            stats: PlanStats {
                links_to_remove: 1,
                ..PlanStats::default()
            },
        };

        let install_plan = ActionPlan {
            actions: vec![Action::CreateLink {
                src: PathBuf::from("/pack/file.txt"),
                dst: PathBuf::from("/target/file.txt"),
                mode: SymlinkMode::Symlink,
            }],
            stats: PlanStats {
                links_to_create: 1,
                ..PlanStats::default()
            },
        };

        let merged = merge_and_dedup(remove_plan, install_plan);

        assert_eq!(merged.actions.len(), 0);
        assert_eq!(merged.stats.links_to_remove, 0);
        assert_eq!(merged.stats.links_to_create, 0);
    }
}
