//! 移除计划生成子模块。
//!
//! 包含 [`plan_remove`] 及其辅助函数。

use std::path::{Path, PathBuf};

use crate::action::{Action, ActionPlan, RemoveDirMode};
use crate::symlink::{Symlink, SymlinkMode};
use crate::track_file::Track;

use super::{VNode, VNodeKind};

/// 为 `track` 中的链接生成移除计划。
///
/// 遍历 track 中每条链接，在 `target_tree` 中查找对应节点，
/// 对比文件系统实际状态与 track 记录是否一致：
/// - 一致 → 生成 `RemoveLink`，从 `target_tree` 中移除该节点
/// - 不一致（drift / 被覆盖 / 类型变化）→ 生成 `Conflict`
/// - 目标已不存在 → 仅累加 `links_to_remove` 统计，不产生文件系统操作
///
/// `state_dir` 为 `$XDG_STATE_HOME/stow-cm/{PACK_ID}/` 目录路径，
/// 传入 `Some` 则在计划末尾追加 `RemoveDir` 彻底清理 pack 状态目录。
pub fn plan_remove(track: &Track, target_tree: &mut VNode, state_dir: Option<&Path>) -> ActionPlan {
    let mut plan = ActionPlan::new();
    let target_root = target_tree.abs_path.clone();

    for link in &track.links {
        // 正常情况 link.dst 一定在 target_root 下（track.target 与 target_tree
        // 来自同一来源），此处仅为防御手动篡改 track 文件的极端情况。
        let Ok(rel) = link.dst.strip_prefix(&target_root) else {
            plan.stats.conflicts += 1;
            plan.actions.push(Action::Conflict {
                dst: link.dst.clone(),
                reason: format!(
                    "link target '{}' is not under the current target root '{}'",
                    link.dst.display(),
                    target_root.display()
                ),
            });
            continue;
        };

        if let Some(node) = target_tree.find(rel) {
            if is_consistent(link, node) {
                plan.actions.push(Action::RemoveLink {
                    src: link.src.clone(),
                    dst: link.dst.clone(),
                    mode: link.mode.clone(),
                });
                plan.stats.links_to_remove += 1;

                // 从 target_tree 中移除该节点，以便收集空目录
                let _ = target_tree.remove(rel);
            } else {
                plan.stats.conflicts += 1;
                plan.actions.push(Action::Conflict {
                    dst: link.dst.clone(),
                    reason: consistency_failure_reason(link, node),
                });
            }
        }
        // 目标不存在：仅累加计数，不产生文件系统操作
    }

    // 存在冲突时执行会被 executor 阻断，跳过清理阶段，
    // 避免计划输出同时出现 CONFLICT 和 RMDIR 造成困惑。
    if plan.stats.conflicts > 0 {
        return plan;
    }

    // 清理移除链接后留下的顶层空目录，同步从虚拟树中移除。
    // 这样后续 plan_install（如 fold）看到的树不再包含这些目录，避免重复生成 RemoveDir。
    let (top_empty_dirs, _) = collect_empty_dirs(target_tree, true);
    for dir in top_empty_dirs {
        plan.actions.push(Action::RemoveDir {
            path: dir.clone(),
            reason: "cleanup empty directory after removal".to_string(),
            mode: RemoveDirMode::IfEmpty,
        });
        plan.stats.dirs_removed += 1;
        // 从虚拟树中移除，防止后续 plan_install 的 fold 对同一目录重复生成 RemoveDir
        if let Ok(rel) = dir.strip_prefix(&target_root) {
            target_tree.remove(rel);
        }
    }

    // 先清理解密目录（可能不在 pack state home 下）
    if let Some(path) = &track.decrypted_path {
        plan.actions.push(Action::RemoveDir {
            path: path.clone(),
            reason: "cleanup decrypted files directory".to_string(),
            mode: RemoveDirMode::All,
        });
        plan.stats.dirs_removed += 1;
    }

    // 再删除整个 pack state 目录（$XDG_STATE_HOME/stow-cm/{PACK_ID}/）
    // 放在最后确保 decrypted_path 内部子目录也一并被 remove_dir_all 兜底清理
    if let Some(dir) = state_dir {
        plan.actions.push(Action::RemoveDir {
            path: dir.to_path_buf(),
            reason: "cleanup pack state directory".to_string(),
            mode: RemoveDirMode::All,
        });
        plan.stats.dirs_removed += 1;
    }

    plan
}

/// 判断 track 中的链接记录与文件系统实际节点是否一致。
fn is_consistent(link: &Symlink, node: &VNode) -> bool {
    match (&link.mode, &node.kind) {
        (SymlinkMode::Symlink, VNodeKind::Symlink { target }) => target == &link.src,
        (SymlinkMode::Copy, VNodeKind::File) => true,
        _ => false,
    }
}

/// 生成一致性检查失败的原因描述。
fn consistency_failure_reason(link: &Symlink, node: &VNode) -> String {
    match (&link.mode, &node.kind) {
        (SymlinkMode::Symlink, VNodeKind::Symlink { target }) => {
            format!(
                "symlink drift: expected target '{}', actual target '{}'",
                link.src.display(),
                target.display()
            )
        }
        (SymlinkMode::Symlink, VNodeKind::File) => {
            format!(
                "expected symlink at '{}', found regular file (overwritten)",
                link.dst.display()
            )
        }
        (SymlinkMode::Symlink, VNodeKind::Dir | VNodeKind::ShallowDir) => {
            format!(
                "expected symlink at '{}', found directory",
                link.dst.display()
            )
        }
        (SymlinkMode::Copy | SymlinkMode::Move, VNodeKind::Symlink { .. }) => {
            format!(
                "expected regular file at '{}', found symlink",
                link.dst.display()
            )
        }
        (SymlinkMode::Copy | SymlinkMode::Move, VNodeKind::Dir | VNodeKind::ShallowDir) => {
            format!(
                "expected regular file at '{}', found directory",
                link.dst.display()
            )
        }
        // 这些分支不会执行（(Copy/Move, File) 在 is_consistent 中返回 true），仅为满足穷尽匹配
        (SymlinkMode::Copy | SymlinkMode::Move, VNodeKind::File) => {
            format!("unexpected inconsistency at '{}'", link.dst.display())
        }
    }
}

/// 收集虚拟树中的顶层空目录：对于嵌套空目录只返回最上层，结果中无父子关系。
///
/// 返回 `(顶层空目录列表, 当前子树是否全空)`。
/// 如果当前子树全空，所有 child 的空目录结果被替换为当前目录自身。
fn collect_empty_dirs(node: &VNode, is_root: bool) -> (Vec<PathBuf>, bool) {
    if node.is_leaf() {
        return (Vec::new(), false);
    }
    let mut top_empty = Vec::new();
    let mut all_children_empty = true;
    for child in &node.children {
        if child.is_leaf() {
            all_children_empty = false;
        } else {
            let (mut sub_empty, sub_all_empty) = collect_empty_dirs(child, false);
            if !sub_all_empty {
                all_children_empty = false;
            }
            top_empty.append(&mut sub_empty);
        }
    }
    if !is_root && all_children_empty {
        // 所有子节点全空：当前目录是顶层空目录，清除子目录的结果
        top_empty.clear();
        top_empty.push(node.abs_path.clone());
    }
    (top_empty, all_children_empty)
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use std::path::PathBuf;

    use super::super::{dir_node, file_node, make_track, symlink_node};
    use super::*;
    use crate::action::Action;
    use crate::symlink::{Symlink, SymlinkMode};

    #[test]
    fn plan_remove_basic() {
        let mock_src = "/pack/file.txt";
        let mock_dst = "/target/file.txt";

        let track = make_track(mock_src, mock_dst);

        // Symlink 模式的 link，target_tree 中应为 Symlink 节点且指向正确的 src
        let mut target_tree = dir_node(
            "",
            "/target",
            vec![symlink_node("file.txt", mock_dst, mock_src)],
        );

        let plan = plan_remove(&track, &mut target_tree, None);

        assert_eq!(plan.stats.links_to_remove, 1);
        assert_eq!(plan.stats.conflicts, 0);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(plan.actions[0], Action::RemoveLink { .. }));
        if let Action::RemoveLink {
            ref src, ref dst, ..
        } = plan.actions[0]
        {
            assert_eq!(src, &PathBuf::from(mock_src));
            assert_eq!(dst, &PathBuf::from(mock_dst));
        }

        // 确认节点已被从 target_tree 中移除
        assert!(target_tree.find(Path::new("file.txt")).is_none());
    }

    #[test]
    fn plan_remove_not_found() {
        // 目标树中没有对应文件 → 不产生操作也不计数
        let track = make_track("/pack/missing.txt", "/target/missing.txt");

        let mut target_tree = dir_node("", "/target", Vec::new());

        let plan = plan_remove(&track, &mut target_tree, None);

        assert_eq!(plan.stats.links_to_remove, 0);
        assert_eq!(plan.stats.conflicts, 0);
        assert_eq!(plan.actions.len(), 0);
    }

    #[test]
    fn plan_remove_multiple_links() {
        let track = Track {
            links: vec![
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
            ],
            decrypted_path: None,
            encrypted: false,
            pack_name: None,
            pack_path: None,
            target: None,
            symlink_mode: None,
        };

        let mut target_tree = dir_node(
            "",
            "/target",
            vec![
                symlink_node("a.txt", "/target/a.txt", "/pack/a.txt"),
                file_node("b.txt", "/target/b.txt"),
            ],
        );

        let plan = plan_remove(&track, &mut target_tree, None);

        assert_eq!(plan.stats.links_to_remove, 2);
        assert_eq!(plan.actions.len(), 2);
        assert!(target_tree.children.is_empty());
    }

    #[test]
    fn plan_remove_symlink_drift_conflict() {
        // track 记录 symlink → src_A，但实际 symlink 指向 src_B → 冲突
        let track = make_track("/pack/a.txt", "/target/file.txt");

        let mut target_tree = dir_node(
            "",
            "/target",
            vec![symlink_node("file.txt", "/target/file.txt", "/other/b.txt")],
        );

        let plan = plan_remove(&track, &mut target_tree, None);

        assert_eq!(plan.stats.conflicts, 1);
        assert_eq!(plan.stats.links_to_remove, 0);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(plan.actions[0], Action::Conflict { .. }));
        if let Action::Conflict {
            ref dst,
            ref reason,
        } = plan.actions[0]
        {
            assert_eq!(dst, &PathBuf::from("/target/file.txt"));
            assert!(
                reason.contains("drift"),
                "reason should mention drift: {reason}"
            );
        }
    }

    #[test]
    fn plan_remove_symlink_overwritten_conflict() {
        // track 记录 symlink，但实际是普通文件（被覆盖）→ 冲突
        let track = make_track("/pack/a.txt", "/target/file.txt");

        let mut target_tree = dir_node(
            "",
            "/target",
            vec![file_node("file.txt", "/target/file.txt")],
        );

        let plan = plan_remove(&track, &mut target_tree, None);

        assert_eq!(plan.stats.conflicts, 1);
        assert_eq!(plan.stats.links_to_remove, 0);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(plan.actions[0], Action::Conflict { .. }));
        if let Action::Conflict { ref reason, .. } = plan.actions[0] {
            assert!(
                reason.contains("overwritten"),
                "reason should mention overwritten: {reason}"
            );
        }
    }

    #[test]
    fn plan_remove_copy_replaced_by_symlink_conflict() {
        // track 记录 copy 模式，但实际是 symlink → 冲突
        let track = Track {
            links: vec![Symlink {
                src: PathBuf::from("/pack/a.txt"),
                dst: PathBuf::from("/target/a.txt"),
                mode: SymlinkMode::Copy,
            }],
            decrypted_path: None,
            encrypted: false,
            pack_name: None,
            pack_path: None,
            target: None,
            symlink_mode: None,
        };

        let mut target_tree = dir_node(
            "",
            "/target",
            vec![symlink_node("a.txt", "/target/a.txt", "/somewhere/else")],
        );

        let plan = plan_remove(&track, &mut target_tree, None);

        assert_eq!(plan.stats.conflicts, 1);
        assert_eq!(plan.stats.links_to_remove, 0);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(plan.actions[0], Action::Conflict { .. }));
    }

    #[test]
    fn plan_remove_out_of_tree_conflict() {
        // link.dst 不在 target_root 下 → 冲突（issue 6）
        let track = make_track("/pack/file.txt", "/other/file.txt");

        let mut target_tree = dir_node("", "/target", Vec::new());

        let plan = plan_remove(&track, &mut target_tree, None);

        assert_eq!(plan.stats.conflicts, 1);
        assert_eq!(plan.stats.links_to_remove, 0);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(plan.actions[0], Action::Conflict { .. }));
        if let Action::Conflict { ref reason, .. } = plan.actions[0] {
            assert!(
                reason.contains("not under"),
                "reason should explain out-of-tree: {reason}"
            );
        }
    }

    #[test]
    fn plan_remove_cleans_empty_dirs() {
        // 安装时创建了子目录结构，移除链接后子目录为空，应生成 RemoveDir
        let mock_src = "/pack/sub/inner.txt";
        let mock_dst = "/target/sub/inner.txt";
        let mock_dir = "/target/sub";

        let track = Track {
            links: vec![Symlink {
                src: PathBuf::from(mock_src),
                dst: PathBuf::from(mock_dst),
                mode: SymlinkMode::Symlink,
            }],
            decrypted_path: None,
            encrypted: false,
            pack_name: None,
            pack_path: None,
            target: None,
            symlink_mode: None,
        };

        let mut target_tree = dir_node(
            "",
            "/target",
            vec![dir_node(
                "sub",
                mock_dir,
                vec![symlink_node("inner.txt", mock_dst, mock_src)],
            )],
        );

        let plan = plan_remove(&track, &mut target_tree, None);

        // 应该移除 1 个链接 + 1 个空目录
        assert_eq!(plan.stats.links_to_remove, 1);
        assert_eq!(plan.stats.dirs_removed, 1);
        assert_eq!(plan.stats.conflicts, 0);

        // 检查操作顺序：RemoveLink 先于 RemoveDir
        assert_eq!(plan.actions.len(), 2);
        assert!(matches!(plan.actions[0], Action::RemoveLink { .. }));
        assert!(matches!(plan.actions[1], Action::RemoveDir { .. }));
        if let Action::RemoveDir { ref path, .. } = plan.actions[1] {
            assert_eq!(path, &PathBuf::from(mock_dir));
        }
    }

    #[test]
    fn plan_remove_no_cleanup_when_dir_has_other_files() {
        // 目录中还有其他文件（非本 pack 所有），不应清理
        let mock_src = "/pack/sub/inner.txt";
        let mock_dst = "/target/sub/inner.txt";
        let mock_dir = "/target/sub";

        let track = Track {
            links: vec![Symlink {
                src: PathBuf::from(mock_src),
                dst: PathBuf::from(mock_dst),
                mode: SymlinkMode::Symlink,
            }],
            decrypted_path: None,
            encrypted: false,
            pack_name: None,
            pack_path: None,
            target: None,
            symlink_mode: None,
        };

        let mut target_tree = dir_node(
            "",
            "/target",
            vec![dir_node(
                "sub",
                mock_dir,
                vec![
                    symlink_node("inner.txt", mock_dst, mock_src),
                    file_node("other.txt", "/target/sub/other.txt"),
                ],
            )],
        );

        let plan = plan_remove(&track, &mut target_tree, None);

        // 移除 1 个链接，但目录不为空（有 other.txt），不生成 RemoveDir
        assert_eq!(plan.stats.links_to_remove, 1);
        assert_eq!(plan.stats.dirs_removed, 0);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(plan.actions[0], Action::RemoveLink { .. }));
    }

    #[test]
    fn plan_remove_cleans_nested_empty_dirs() {
        // 嵌套空目录：a/b/inner.txt → 移除后 a/b 和 a 都为空 → 只删顶层 a
        let mock_src = "/pack/a/b/inner.txt";
        let mock_dst = "/target/a/b/inner.txt";
        let dir_b = "/target/a/b";
        let dir_a = "/target/a";

        let track = Track {
            links: vec![Symlink {
                src: PathBuf::from(mock_src),
                dst: PathBuf::from(mock_dst),
                mode: SymlinkMode::Symlink,
            }],
            decrypted_path: None,
            encrypted: false,
            pack_name: None,
            pack_path: None,
            target: None,
            symlink_mode: None,
        };

        let mut target_tree = dir_node(
            "",
            "/target",
            vec![dir_node(
                "a",
                dir_a,
                vec![dir_node(
                    "b",
                    dir_b,
                    vec![symlink_node("inner.txt", mock_dst, mock_src)],
                )],
            )],
        );

        let plan = plan_remove(&track, &mut target_tree, None);

        // 1 个链接 + 1 个顶层空目录（a 包含了 b，递归删除即可）
        assert_eq!(plan.stats.links_to_remove, 1);
        assert_eq!(plan.stats.dirs_removed, 1);
        assert_eq!(plan.stats.conflicts, 0);
        assert_eq!(plan.actions.len(), 2);

        let remove_dirs: Vec<_> = plan
            .actions
            .iter()
            .filter_map(|a| {
                if let Action::RemoveDir { path, .. } = a {
                    Some(path.clone())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(remove_dirs, vec![PathBuf::from(dir_a)]);
    }

    #[test]
    fn plan_remove_no_cleanup_of_target_root() {
        // 即使根目录在移除后为空，也不应清理（根目录是用户的 target 目录）
        let mock_src = "/pack/file.txt";
        let mock_dst = "/target/file.txt";

        let track = Track {
            links: vec![Symlink {
                src: PathBuf::from(mock_src),
                dst: PathBuf::from(mock_dst),
                mode: SymlinkMode::Symlink,
            }],
            decrypted_path: None,
            encrypted: false,
            pack_name: None,
            pack_path: None,
            target: None,
            symlink_mode: None,
        };

        let mut target_tree = dir_node(
            "",
            "/target",
            vec![symlink_node("file.txt", mock_dst, mock_src)],
        );

        let plan = plan_remove(&track, &mut target_tree, None);

        // 移除 1 个链接，根目录 "/target" 不应被清理
        assert_eq!(plan.stats.links_to_remove, 1);
        assert_eq!(plan.stats.dirs_removed, 0);
        assert_eq!(plan.actions.len(), 1);
    }
}
