//! 安装计划生成子模块。
//!
//! 包含 [`plan_install`] 及其递归核心：`install_children` → `install_node` →
//! `plan_leaf` → `plan_dir`。

use std::path::Path;

use crate::action::{Action, ActionPlan, PlanStats, RemoveDirMode};
use crate::error::Result;
use crate::symlink::{Symlink, SymlinkMode};
use crate::track_file::Track;
use crate::util;

use super::{ChildrenPlan, PlanOption, VNode, VNodeKind};

/// 为 `pack_tree` 生成安装计划。
///
/// 递归比较 `pack_tree` 与 `target_tree`，为每个 pack 叶子节点生成
/// `CreateLink` 操作。遇到冲突时生成 `Conflict`，匹配忽略规则时跳过，
/// 满足覆盖规则时强制覆盖。支持目录折叠（fold）优化。
///
/// 同步更新 `target_tree`（虚拟文件树），使其反映计划执行后的预期状态。
///
/// 如果 `options.decrypt` 已设置，仅对内容含 `left_boundary` 占位符的文件
/// 生成 `DecryptFile` 并把 `CreateLink.src` 重写为解密后的路径；其余文件保持
/// 直接链接 pack 原文件（保留 symlink“改源即生效”的语义，且不额外复制）。
///
/// 如果 `track_write` 已设置，会额外生成：
/// - `CreateDir(decrypted_path)`（确有文件需要解密时）
/// - `WriteTrackFile` 写入安装后的 track 记录
pub fn plan_install(
    pack_tree: &mut VNode,
    target_tree: &mut VNode,
    options: &PlanOption,
) -> Result<ActionPlan> {
    let target_base = target_tree.abs_path.clone();
    let children_plan = install_children(
        &mut pack_tree.children,
        &mut target_tree.children,
        &target_base,
        options,
    )?;
    let mut plan = ActionPlan {
        actions: children_plan.actions,
        stats: *children_plan.stats,
    };

    // 解密目录：只有确实生成了 DecryptFile 才需要预先创建顶层目录
    // （逐文件的父目录由 executor 兜底创建），避免为全普通文件的加密包建空目录。
    if let Some(decrypt) = &options.decrypt
        && plan
            .actions
            .iter()
            .any(|a| matches!(a, Action::DecryptFile { .. }))
    {
        plan.actions
            .insert(0, Action::CreateDir(decrypt.decrypted_path.clone()));
        plan.stats.dirs_to_create += 1;
    }

    // 注入 track file 写入
    if let Some(tw) = &options.track_write {
        let symlinks: Vec<Symlink> = plan
            .actions
            .iter()
            .filter_map(|a| match a {
                Action::CreateLink { src, dst, mode } => Some(Symlink {
                    src: src.clone(),
                    dst: dst.clone(),
                    mode: mode.clone(),
                }),
                _ => None,
            })
            .collect();

        plan.actions.push(Action::WriteTrackFile {
            path: tw.track_file.clone(),
            track: Track {
                decrypted_path: options.decrypt.as_ref().map(|d| d.decrypted_path.clone()),
                encrypted: tw.encrypted,
                links: symlinks,
                pack_name: Some(tw.pack_name.clone()),
                pack_path: Some(tw.pack_path.clone()),
                target: Some(tw.target.clone()),
                symlink_mode: tw.symlink_mode.clone(),
            },
        });
    }

    Ok(plan)
}

// ── 递归安装核心 ──

/// 递归处理 pack 的一组子节点，将每个子节点安装到 target 对应位置。
/// 合并处理传播标记, 但不做 fold 实际操作，fold 在 `plan_dir` 中处理
#[allow(clippy::indexing_slicing)]
fn install_children(
    pack_children: &mut Vec<VNode>,
    target_children: &mut Vec<VNode>,
    target_base: &Path,
    options: &PlanOption,
) -> Result<ChildrenPlan> {
    let mut result = ChildrenPlan::empty();

    // 检查 target 中是否存在 pack 没有的子节点（外部文件），需在迭代前判断，
    // 因为 Move 模式下 plan_leaf 会从 pack_children 中移除节点。
    let has_new_sub = target_children
        .iter()
        .any(|tc| !pack_children.iter().any(|pc| pc.rel_path == tc.rel_path));

    let mut i = pack_children.len();
    while i > 0 {
        i -= 1;
        let idx = target_children
            .iter()
            .position(|tc| tc.rel_path == pack_children[i].rel_path);

        let child_target_base = target_base.join(&pack_children[i].rel_path);

        let child_plan = if let Some(target_idx) = idx {
            install_node(
                pack_children,
                i,
                target_children,
                Some(target_idx),
                &child_target_base,
                options,
            )?
        } else {
            install_node(
                pack_children,
                i,
                target_children,
                None,
                &child_target_base,
                options,
            )?
        };

        // 合并统计信息
        result.stats.links_to_create += child_plan.stats.links_to_create;
        result.stats.conflicts += child_plan.stats.conflicts;
        result.stats.ignored += child_plan.stats.ignored;
        result.stats.overridden += child_plan.stats.overridden;
        result.stats.encrypted += child_plan.stats.encrypted;
        result.stats.dirs_removed += child_plan.stats.dirs_removed;
        result.stats.files_removed += child_plan.stats.files_removed;

        if child_plan.had_ignored {
            result.had_ignored = true;
        }
        if child_plan.needs_decrypt {
            result.needs_decrypt = true;
        }
        if !child_plan.foldable {
            result.foldable = false;
        }

        result.actions.extend(child_plan.actions);
    }

    // 负责 foldable 传播：
    // - 有被忽略的子节点 / target 存在外部文件时不可折叠
    // - 子树中存在需要解密的文件时不可折叠
    //   （折叠会把整个目录做成单个 symlink，解密会被整体跳过，见 BUG-1）
    result.foldable =
        result.foldable && !result.had_ignored && !has_new_sub && !result.needs_decrypt;
    Ok(result)
}

/// - `pack_parent` / `pack_idx` — pack 节点在其父列表中的位置（Move 模式 possibly remove）
/// - `target_parent` / `target_idx` — 目标节点在目标父列表中的位置（None 表示不存在）
/// - `target_dst` — 该 pack 节点在目标文件系统中的预期绝对路径
#[allow(clippy::indexing_slicing)]
fn install_node(
    pack_parent: &mut Vec<VNode>,
    pack_idx: usize,
    target_parent: &mut Vec<VNode>,
    target_idx: Option<usize>,
    target_dst: &Path,
    options: &PlanOption,
) -> Result<ChildrenPlan> {
    let pack = &pack_parent[pack_idx];

    // ── 忽略检查 ──
    if let Some(ignore_re) = &options.merge.ignore
        && ignore_re.is_match(&pack.abs_path.to_string_lossy())
    {
        let mut stats = Box::<PlanStats>::default();
        stats.ignored = 1;
        return Ok(ChildrenPlan {
            actions: Vec::new(),
            stats,
            foldable: false,
            had_ignored: true,
            needs_decrypt: false,
        });
    }

    match &pack.kind {
        VNodeKind::File | VNodeKind::Symlink { .. } => plan_leaf(
            pack_parent,
            pack_idx,
            target_parent,
            target_idx,
            target_dst,
            options,
        ),
        VNodeKind::Dir => plan_dir(
            pack_parent,
            pack_idx,
            target_parent,
            target_idx,
            target_dst,
            options,
        ),
        // ShallowDir 不会出现在 pack 树中（pack 使用完整扫描），仅为穷尽匹配
        VNodeKind::ShallowDir => unreachable!("ShallowDir should not appear in pack tree"),
    }
}

/// 处理叶子节点（File 或 Symlink）。
///
/// 操作虚拟文件树：
/// - Move 模式：`pack_parent.remove(pack_idx)` 从 pack 父节点移除
/// - 覆盖时：`target_parent[target_idx] = new_node` 替换目标节点
/// - 新建时：`target_parent.push(new_node)` 插入目标节点
///
/// 启用解密时，仅当配置开启了加密且文件内容含占位符
/// （[`util::file_has_placeholder`]）才生成 `DecryptFile` 并把链接指向解密副本；
/// 其余文件直接链接 pack 原文件，保留 symlink“改源即生效”的语义且不额外复制。
#[allow(clippy::indexing_slicing)]
fn plan_leaf(
    pack_parent: &mut Vec<VNode>,
    pack_idx: usize,
    target_parent: &mut Vec<VNode>,
    target_idx: Option<usize>,
    target_dst: &Path,
    options: &PlanOption,
) -> Result<ChildrenPlan> {
    let mode = options.merge.symlink_mode.clone().unwrap_or_default();

    // 提取 pack 节点信息（clone），释放对 pack_parent 的 immutable borrow
    let pack_abs = pack_parent[pack_idx].abs_path.clone();
    let pack_kind = pack_parent[pack_idx].kind.clone();
    let pack_rel = pack_parent[pack_idx].rel_path.clone();

    // Move 模式：从 pack 父节点中移除当前节点
    if mode == SymlinkMode::Move {
        pack_parent.remove(pack_idx);
    }

    // 只有「加密配置已开启」且「文件内容含完整占位符」时才需要解密，
    // 其余文件（含未开启加密时的所有文件）都直接链接 pack 原文件。
    let decrypt = options.decrypt.as_ref().filter(|decrypt| {
        util::file_has_placeholder(&pack_abs, &decrypt.left_boundary, &decrypt.right_boundary)
    });
    let needs_decrypt = decrypt.is_some();
    let (link_src, decrypt_action) = if let Some(decrypt) = decrypt {
        let to = util::change_base_path(&pack_abs, &decrypt.pack_path, &decrypt.decrypted_path)?;
        let action = Action::DecryptFile {
            src: pack_abs.clone(),
            to: to.clone(),
            key: decrypt.key.clone(),
            alg: decrypt.alg.clone(),
            left_boundary: decrypt.left_boundary.clone(),
            right_boundary: decrypt.right_boundary.clone(),
        };
        (to, Some(action))
    } else {
        (pack_abs.clone(), None)
    };

    if let Some(target_idx) = target_idx {
        // 目标已存在 — 检查是否可覆盖
        if let Some(over_re) = &options.merge.over
            && over_re.is_match(&pack_abs.to_string_lossy())
        {
            // 记录旧目标类型，用于生成清理 action
            let target_kind = target_parent[target_idx].kind.clone();
            let dst = target_parent[target_idx].abs_path.clone();

            let kind = match &mode {
                SymlinkMode::Symlink => VNodeKind::Symlink {
                    target: link_src.clone(),
                },
                SymlinkMode::Copy | SymlinkMode::Move => pack_kind.clone(),
            };
            target_parent[target_idx] = VNode {
                rel_path: target_parent[target_idx].rel_path.clone(),
                abs_path: dst.clone(),
                kind,
                children: Vec::new(),
            };

            let mut stats = Box::<PlanStats>::default();
            stats.links_to_create = 1;
            stats.overridden = 1;
            stats.encrypted = usize::from(needs_decrypt);

            let mut actions = Vec::new();
            // 覆盖前先清理旧目标
            match target_kind {
                VNodeKind::Dir | VNodeKind::ShallowDir => {
                    actions.push(Action::RemoveDir {
                        path: dst.clone(),
                        reason: "overridden".to_string(),
                        mode: RemoveDirMode::All,
                    });
                    stats.dirs_removed = 1;
                }
                VNodeKind::File | VNodeKind::Symlink { .. } => {
                    actions.push(Action::RemoveFile {
                        path: dst.clone(),
                        reason: "overridden".to_string(),
                    });
                    stats.files_removed = 1;
                }
            }
            // 先解密再创建链接
            actions.extend(decrypt_action);
            actions.push(Action::CreateLink {
                src: link_src,
                dst,
                mode,
            });

            return Ok(ChildrenPlan {
                actions,
                stats,
                foldable: true,
                had_ignored: false,
                needs_decrypt,
            });
        }
        // 检查是否是事实上的同一文件（同一 inode），如果是则无需操作
        if util::same_file(&pack_abs, &target_parent[target_idx].abs_path) {
            return Ok(ChildrenPlan {
                actions: Vec::new(),
                stats: Box::default(),
                foldable: true,
                had_ignored: false,
                needs_decrypt,
            });
        }

        // 存在且不可覆盖 → 冲突，不可折叠
        let mut stats = Box::<PlanStats>::default();
        stats.conflicts = 1;
        Ok(ChildrenPlan {
            actions: vec![Action::Conflict {
                dst: target_parent[target_idx].abs_path.clone(),
                reason: "file already exists".to_string(),
            }],
            stats,
            foldable: false,
            had_ignored: false,
            needs_decrypt,
        })
    } else {
        // 目标不存在 → 正常创建链接
        let kind = match &mode {
            SymlinkMode::Symlink => VNodeKind::Symlink {
                target: link_src.clone(),
            },
            SymlinkMode::Copy | SymlinkMode::Move => pack_kind.clone(),
        };
        target_parent.push(VNode {
            rel_path: pack_rel,
            abs_path: target_dst.to_path_buf(),
            kind,
            children: Vec::new(),
        });

        let mut stats = Box::<PlanStats>::default();
        stats.links_to_create = 1;
        stats.encrypted = usize::from(needs_decrypt);

        let mut actions = Vec::new();
        // 先解密再创建链接
        actions.extend(decrypt_action);
        actions.push(Action::CreateLink {
            src: link_src,
            dst: target_dst.to_path_buf(),
            mode,
        });

        Ok(ChildrenPlan {
            actions,
            stats,
            foldable: true,
            had_ignored: false,
            needs_decrypt,
        })
    }
}

/// 处理目录节点：递归子节点，检查折叠条件，并折叠。
///
/// 操作虚拟文件树：
/// - 折叠时替换 `target_parent[target_idx]` 或 push 到 `target_parent`
/// - Move 模式 `pack_parent.remove(pack_idx)`
/// - 非折叠时递归处理子节点
#[allow(clippy::indexing_slicing)]
fn plan_dir(
    pack_parent: &mut Vec<VNode>,
    pack_idx: usize,
    target_parent: &mut Vec<VNode>,
    target_idx: Option<usize>,
    target_dst: &Path,
    options: &PlanOption,
) -> Result<ChildrenPlan> {
    let mode = options.merge.symlink_mode.clone().unwrap_or_default();
    let fold_enabled = options.merge.fold.unwrap_or(false);

    // 提取 pack 节点信息（clone），释放对 pack_parent 的 immutable borrow
    let pack_abs = pack_parent[pack_idx].abs_path.clone();
    let pack_kind = pack_parent[pack_idx].kind.clone();
    let pack_rel = pack_parent[pack_idx].rel_path.clone();

    // 提取 target_children 供递归处理，用独立作用域限制 borrow 生命周期
    let children_plan = {
        let mut empty_children = Vec::new();
        let target_children: &mut Vec<VNode> =
            target_idx.map_or(&mut empty_children, |idx| &mut target_parent[idx].children);
        install_children(
            &mut pack_parent[pack_idx].children,
            target_children,
            target_dst,
            options,
        )?
    };

    // 负责能不能 fold。
    // 含占位符、需要逐文件解密的目录已由 install_children 通过 foldable=false 排除
    // （折叠会把整个目录做成单个 symlink，解密会被整体跳过，见 BUG-1），
    // 因此启用加密不再与 fold 整体互斥，仅含普通文件的目录仍可折叠。
    let should_fold = fold_enabled && mode != SymlinkMode::Copy && children_plan.foldable;

    if should_fold {
        // Move 模式：从 pack 父节点中移除整个目录
        if mode == SymlinkMode::Move {
            pack_parent.remove(pack_idx);
        }

        let dst = target_idx.map_or_else(
            || target_dst.to_path_buf(),
            |idx| target_parent[idx].abs_path.clone(),
        );

        let mut actions = Vec::new();
        let mut stats = Box::<PlanStats>::default();

        if let Some(idx) = target_idx {
            actions.push(Action::RemoveDir {
                path: dst.clone(),
                reason: "folded directory replaced".to_string(),
                mode: RemoveDirMode::All,
            });
            stats.dirs_removed = 1;

            let kind = match &mode {
                SymlinkMode::Symlink => VNodeKind::Symlink {
                    target: pack_abs.clone(),
                },
                SymlinkMode::Copy | SymlinkMode::Move => pack_kind.clone(),
            };
            target_parent[idx] = VNode {
                rel_path: target_parent[idx].rel_path.clone(),
                abs_path: dst.clone(),
                kind,
                children: Vec::new(),
            };
        } else {
            let kind = match &mode {
                SymlinkMode::Symlink => VNodeKind::Symlink {
                    target: pack_abs.clone(),
                },
                SymlinkMode::Copy | SymlinkMode::Move => pack_kind.clone(),
            };
            target_parent.push(VNode {
                rel_path: pack_rel,
                abs_path: dst.clone(),
                kind,
                children: Vec::new(),
            });
        }

        actions.push(Action::CreateLink {
            src: pack_abs,
            dst,
            mode,
        });
        stats.links_to_create = 1;

        Ok(ChildrenPlan {
            actions,
            stats,
            foldable: true,
            had_ignored: false,
            needs_decrypt: false,
        })
    } else {
        Ok(children_plan)
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::super::{
        MergeOption, default_options, dir_node, file_node, fold_options, symlink_node,
    };
    use super::*;
    use crate::action::Action;
    use crate::planner::DecryptOption;
    use crate::symlink::SymlinkMode;
    use crate::vtree;

    #[test]
    fn plan_install_basic_file() {
        // pack: /pack/file.txt  →  target: (empty)
        let mut pack = dir_node("", "/pack", vec![file_node("file.txt", "/pack/file.txt")]);
        let mut target = dir_node("", "/target", Vec::new());

        let plan = plan_install(&mut pack, &mut target, &default_options()).unwrap();

        assert_eq!(plan.stats.links_to_create, 1);
        assert_eq!(plan.stats.conflicts, 0);
        assert_eq!(plan.stats.ignored, 0);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(plan.actions[0], Action::CreateLink { .. }));
        if let Action::CreateLink {
            ref src, ref dst, ..
        } = plan.actions[0]
        {
            assert_eq!(src, &PathBuf::from("/pack/file.txt"));
            assert_eq!(dst, &PathBuf::from("/target/file.txt"));
        }
    }

    #[test]
    fn plan_install_multiple_files() {
        let mut pack = dir_node(
            "",
            "/pack",
            vec![
                file_node("a.txt", "/pack/a.txt"),
                file_node("b.txt", "/pack/b.txt"),
            ],
        );
        let mut target = dir_node("", "/target", Vec::new());

        let plan = plan_install(&mut pack, &mut target, &default_options()).unwrap();

        assert_eq!(plan.stats.links_to_create, 2);
        assert_eq!(plan.actions.len(), 2);
    }

    #[test]
    fn plan_install_conflict() {
        // target 中已存在同名文件 → 冲突
        let mut pack = dir_node("", "/pack", vec![file_node("file.txt", "/pack/file.txt")]);
        let mut target = dir_node(
            "",
            "/target",
            vec![file_node("file.txt", "/target/file.txt")],
        );

        let plan = plan_install(&mut pack, &mut target, &default_options()).unwrap();

        assert_eq!(plan.stats.conflicts, 1);
        assert_eq!(plan.stats.links_to_create, 0);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(plan.actions[0], Action::Conflict { .. }));
    }

    #[test]
    fn plan_install_override() {
        // over 规则匹配 → 强制覆盖
        let mut pack = dir_node("", "/pack", vec![file_node("file.txt", "/pack/file.txt")]);
        let mut target = dir_node(
            "",
            "/target",
            vec![file_node("file.txt", "/target/file.txt")],
        );

        let options = PlanOption {
            merge: MergeOption {
                ignore: None,
                over: Some(regex::RegexSet::new([".*file\\.txt"]).unwrap()),
                fold: None,
                symlink_mode: None,
            },
            decrypt: None,
            track_write: None,
        };

        let plan = plan_install(&mut pack, &mut target, &options).unwrap();

        assert_eq!(plan.stats.overridden, 1);
        assert_eq!(plan.stats.conflicts, 0);
        assert_eq!(plan.stats.links_to_create, 1);
        assert_eq!(plan.stats.files_removed, 1);
        assert_eq!(plan.actions.len(), 2);
        assert!(matches!(plan.actions[0], Action::RemoveFile { .. }));
        assert!(matches!(plan.actions[1], Action::CreateLink { .. }));
    }

    #[test]
    fn plan_install_ignore() {
        // ignore 规则匹配 → 跳过
        let mut pack = dir_node(
            "",
            "/pack",
            vec![
                file_node("readme.md", "/pack/readme.md"),
                file_node("config.txt", "/pack/config.txt"),
            ],
        );
        let mut target = dir_node("", "/target", Vec::new());

        let options = PlanOption {
            merge: MergeOption {
                ignore: Some(regex::RegexSet::new([".*\\.md"]).unwrap()),
                over: None,
                fold: None,
                symlink_mode: None,
            },
            decrypt: None,
            track_write: None,
        };

        let plan = plan_install(&mut pack, &mut target, &options).unwrap();

        assert_eq!(plan.stats.ignored, 1);
        assert_eq!(plan.stats.links_to_create, 1);
        assert_eq!(plan.actions.len(), 1);
        if let Action::CreateLink { ref src, .. } = plan.actions[0] {
            assert_eq!(src, &PathBuf::from("/pack/config.txt"));
        }
    }

    #[test]
    fn plan_install_nested_dir() {
        // pack 中有嵌套目录结构
        let mut pack = dir_node(
            "",
            "/pack",
            vec![dir_node(
                "subdir",
                "/pack/subdir",
                vec![file_node("inner.txt", "/pack/subdir/inner.txt")],
            )],
        );
        let mut target = dir_node("", "/target", Vec::new());

        let plan = plan_install(&mut pack, &mut target, &default_options()).unwrap();

        assert_eq!(plan.stats.links_to_create, 1);
        assert_eq!(plan.actions.len(), 1);
        if let Action::CreateLink {
            ref src, ref dst, ..
        } = plan.actions[0]
        {
            assert_eq!(src, &PathBuf::from("/pack/subdir/inner.txt"));
            assert_eq!(dst, &PathBuf::from("/target/subdir/inner.txt"));
        }
    }

    #[test]
    fn plan_install_fold() {
        // 启用 fold：可折叠的目录 → 单个 CreateLink
        let mut pack = dir_node(
            "",
            "/pack",
            vec![dir_node(
                "sub",
                "/pack/sub",
                vec![
                    file_node("x.txt", "/pack/sub/x.txt"),
                    file_node("y.txt", "/pack/sub/y.txt"),
                ],
            )],
        );
        let mut target = dir_node("", "/target", Vec::new());

        let plan = plan_install(&mut pack, &mut target, &fold_options()).unwrap();

        // fold 后：整个 sub 目录折叠为一个链接
        assert_eq!(plan.stats.links_to_create, 1);
        assert_eq!(plan.actions.len(), 1);
        if let Action::CreateLink {
            ref src, ref dst, ..
        } = plan.actions[0]
        {
            assert_eq!(src, &PathBuf::from("/pack/sub"));
            assert_eq!(dst, &PathBuf::from("/target/sub"));
        }
    }

    #[test]
    fn plan_install_fold_blocked_by_ignore() {
        // 目录中有被忽略的文件 → 不可折叠
        let mut pack = dir_node(
            "",
            "/pack",
            vec![dir_node(
                "sub",
                "/pack/sub",
                vec![
                    file_node("readme.md", "/pack/sub/readme.md"),
                    file_node("x.txt", "/pack/sub/x.txt"),
                ],
            )],
        );
        let mut target = dir_node("", "/target", Vec::new());

        let options = PlanOption {
            merge: MergeOption {
                ignore: Some(regex::RegexSet::new([".*\\.md"]).unwrap()),
                over: None,
                fold: Some(true),
                symlink_mode: None,
            },
            decrypt: None,
            track_write: None,
        };

        let plan = plan_install(&mut pack, &mut target, &options).unwrap();

        // 不可折叠 → 子节点单独链接
        assert_eq!(plan.stats.links_to_create, 1);
        assert_eq!(plan.stats.ignored, 1);
        assert_eq!(plan.actions.len(), 1);
        if let Action::CreateLink { ref src, .. } = plan.actions[0] {
            assert_eq!(src, &PathBuf::from("/pack/sub/x.txt"));
        }
    }

    #[test]
    fn plan_install_fold_blocked_by_new_sub() {
        // target 中存在 pack 没有的文件 → 不可折叠
        let mut pack = dir_node(
            "",
            "/pack",
            vec![dir_node(
                "sub",
                "/pack/sub",
                vec![file_node("x.txt", "/pack/sub/x.txt")],
            )],
        );
        let mut target = dir_node(
            "",
            "/target",
            vec![dir_node(
                "sub",
                "/target/sub",
                vec![file_node("extra.txt", "/target/sub/extra.txt")],
            )],
        );

        let plan = plan_install(&mut pack, &mut target, &fold_options()).unwrap();

        // 不可折叠：target 中有 extra.txt
        assert_eq!(plan.actions.len(), 1);
        if let Action::CreateLink { ref src, .. } = plan.actions[0] {
            assert_eq!(src, &PathBuf::from("/pack/sub/x.txt"));
        }
    }

    #[test]
    fn plan_install_symlink_node() {
        // Symlink 类型的节点应被视为叶子，生成 CreateLink
        let mut pack = dir_node(
            "",
            "/pack",
            vec![symlink_node("link.txt", "/pack/link.txt", "/etc/somefile")],
        );
        let mut target = dir_node("", "/target", Vec::new());

        let plan = plan_install(&mut pack, &mut target, &default_options()).unwrap();

        assert_eq!(plan.stats.links_to_create, 1);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(plan.actions[0], Action::CreateLink { .. }));
    }

    #[test]
    fn plan_install_same_file_no_conflict() {
        let dir = tempfile::TempDir::with_prefix("stow-cm-test-").unwrap();
        let dir_str = dir.path().to_str().unwrap();
        let real_file = dir.path().join("realfile.txt");
        std::fs::write(&real_file, "content").unwrap();

        std::fs::create_dir(dir.path().join("subdir")).unwrap();

        let pack_abs = format!("{dir_str}/subdir/../realfile.txt");
        let target_abs = format!("{dir_str}/realfile.txt");

        let mut pack = dir_node("", dir_str, vec![file_node("realfile.txt", &pack_abs)]);
        let mut target = dir_node("", dir_str, vec![file_node("realfile.txt", &target_abs)]);

        let plan = plan_install(&mut pack, &mut target, &default_options()).unwrap();

        assert_eq!(plan.stats.conflicts, 0);
        assert_eq!(plan.stats.links_to_create, 0);
        assert_eq!(plan.actions.len(), 0);
    }

    #[test]
    fn plan_install_copy_mode_no_fold() {
        // copy 模式下即使启用 fold，目录也不应折叠（每个文件单独 CreateLink）
        let mut pack = dir_node(
            "",
            "/pack",
            vec![dir_node(
                "sub",
                "/pack/sub",
                vec![
                    file_node("x.txt", "/pack/sub/x.txt"),
                    file_node("y.txt", "/pack/sub/y.txt"),
                ],
            )],
        );
        let mut target = dir_node("", "/target", Vec::new());

        let options = PlanOption {
            merge: MergeOption {
                ignore: None,
                over: None,
                fold: Some(true),
                symlink_mode: Some(SymlinkMode::Copy),
            },
            decrypt: None,
            track_write: None,
        };

        let plan = plan_install(&mut pack, &mut target, &options).unwrap();

        // copy 模式不折叠：应该为每个文件生成单独的 CreateLink
        assert_eq!(plan.stats.links_to_create, 2);
        assert_eq!(plan.actions.len(), 2);
        for action in &plan.actions {
            if let Action::CreateLink { mode, .. } = action {
                assert_eq!(*mode, SymlinkMode::Copy);
            } else {
                panic!("expected CreateLink, got {action:?}");
            }
        }
    }

    #[test]
    fn plan_install_copy_mode_no_fold_single_file() {
        // copy 模式下单个文件的目录：折叠不影响，因为是叶子
        let mut pack = dir_node(
            "",
            "/pack",
            vec![file_node("single.txt", "/pack/single.txt")],
        );
        let mut target = dir_node("", "/target", Vec::new());

        let options = PlanOption {
            merge: MergeOption {
                ignore: None,
                over: None,
                fold: Some(true),
                symlink_mode: Some(SymlinkMode::Copy),
            },
            decrypt: None,
            track_write: None,
        };

        let plan = plan_install(&mut pack, &mut target, &options).unwrap();

        assert_eq!(plan.stats.links_to_create, 1);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(plan.actions[0], Action::CreateLink { .. }));
        if let Action::CreateLink { mode, .. } = &plan.actions[0] {
            assert_eq!(*mode, SymlinkMode::Copy);
        }
    }

    #[test]
    fn plan_install_move_mode_allows_fold() {
        // move 模式允许 fold
        let mut pack = dir_node(
            "",
            "/pack",
            vec![dir_node(
                "sub",
                "/pack/sub",
                vec![
                    file_node("x.txt", "/pack/sub/x.txt"),
                    file_node("y.txt", "/pack/sub/y.txt"),
                ],
            )],
        );
        let mut target = dir_node("", "/target", Vec::new());

        let options = PlanOption {
            merge: MergeOption {
                ignore: None,
                over: None,
                fold: Some(true),
                symlink_mode: Some(SymlinkMode::Move),
            },
            decrypt: None,
            track_write: None,
        };

        let plan = plan_install(&mut pack, &mut target, &options).unwrap();

        // move 模式允许 fold：整个 sub 目录折叠为一个 CreateLink
        assert_eq!(plan.stats.links_to_create, 1);
        assert_eq!(plan.actions.len(), 1);
        if let Action::CreateLink {
            ref src, ref mode, ..
        } = plan.actions[0]
        {
            assert_eq!(src, &PathBuf::from("/pack/sub"));
            assert_eq!(*mode, SymlinkMode::Move);
        }
    }

    // ── 解密（占位符检测）──

    /// 构造启用解密、fold 打开的选项。
    fn decrypt_options(pack: &Path, decrypted: &Path) -> PlanOption {
        PlanOption {
            merge: MergeOption {
                ignore: None,
                over: None,
                fold: Some(true),
                symlink_mode: None,
            },
            decrypt: Some(DecryptOption {
                decrypted_path: decrypted.to_path_buf(),
                key: vec![0u8; 32],
                alg: "ChaCha20-Poly1305".to_string(),
                left_boundary: "&{".to_string(),
                right_boundary: "}".to_string(),
                pack_path: pack.to_path_buf(),
            }),
            track_write: None,
        }
    }

    /// 返回指向 `dst` 的 `CreateLink.src`。
    fn link_src(plan: &ActionPlan, dst: &Path) -> PathBuf {
        plan.actions
            .iter()
            .find_map(|a| match a {
                Action::CreateLink { src, dst: d, .. } if d == dst => Some(src.clone()),
                _ => None,
            })
            .expect("no CreateLink for the given dst")
    }

    fn has_create_link_to(plan: &ActionPlan, dst: &Path) -> bool {
        plan.actions
            .iter()
            .any(|a| matches!(a, Action::CreateLink { dst: d, .. } if d == dst))
    }

    #[test]
    fn plan_install_decrypt_only_placeholder_files() {
        let dir = tempfile::TempDir::with_prefix("stow-cm-test-").unwrap();
        let pack = dir.path().join("pack");
        let target = dir.path().join("target");
        let decrypted = dir.path().join("decrypted");
        std::fs::create_dir_all(&pack).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(pack.join("secret.txt"), "public &{secret} public\n").unwrap();
        std::fs::write(pack.join("plain.txt"), "no markers here\n").unwrap();

        let mut pack_tree = vtree::VNode::scan(&pack, false).unwrap();
        let mut target_tree = vtree::VNode::scan_guided(&target, &pack_tree, false).unwrap();
        let plan = plan_install(
            &mut pack_tree,
            &mut target_tree,
            &decrypt_options(&pack, &decrypted),
        )
        .unwrap();

        // 只有占位符文件生成 DecryptFile
        assert_eq!(plan.stats.encrypted, 1);
        assert_eq!(
            plan.actions
                .iter()
                .filter(|a| matches!(a, Action::DecryptFile { .. }))
                .count(),
            1
        );
        // 解密目录按需创建
        assert!(matches!(plan.actions.first(), Some(Action::CreateDir(_))));

        assert_eq!(
            link_src(&plan, &target.join("secret.txt")),
            decrypted.join("secret.txt")
        );
        // 普通文件直接链接 pack 原文件
        assert_eq!(
            link_src(&plan, &target.join("plain.txt")),
            pack.join("plain.txt")
        );
    }

    #[test]
    fn plan_install_decrypt_ignores_unclosed_placeholder() {
        let dir = tempfile::TempDir::with_prefix("stow-cm-test-").unwrap();
        let pack = dir.path().join("pack");
        let target = dir.path().join("target");
        let decrypted = dir.path().join("decrypted");
        std::fs::create_dir_all(&pack).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        // 只有左边界、没有右边界 → 不是完整占位符，无需解密
        std::fs::write(pack.join("dangling.txt"), "a &{no close\n").unwrap();

        let mut pack_tree = vtree::VNode::scan(&pack, false).unwrap();
        let mut target_tree = vtree::VNode::scan_guided(&target, &pack_tree, false).unwrap();
        let plan = plan_install(
            &mut pack_tree,
            &mut target_tree,
            &decrypt_options(&pack, &decrypted),
        )
        .unwrap();

        assert_eq!(plan.stats.encrypted, 0);
        assert!(
            !plan
                .actions
                .iter()
                .any(|a| matches!(a, Action::DecryptFile { .. }))
        );
        assert!(
            !plan
                .actions
                .iter()
                .any(|a| matches!(a, Action::CreateDir(_)))
        );
        assert_eq!(
            link_src(&plan, &target.join("dangling.txt")),
            pack.join("dangling.txt")
        );
    }

    #[test]
    fn plan_install_decrypt_plain_only_skips_decrypted_dir() {
        let dir = tempfile::TempDir::with_prefix("stow-cm-test-").unwrap();
        let pack = dir.path().join("pack");
        let target = dir.path().join("target");
        let decrypted = dir.path().join("decrypted");
        std::fs::create_dir_all(&pack).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(pack.join("plain.txt"), "no markers here\n").unwrap();

        let mut pack_tree = vtree::VNode::scan(&pack, false).unwrap();
        let mut target_tree = vtree::VNode::scan_guided(&target, &pack_tree, false).unwrap();
        let plan = plan_install(
            &mut pack_tree,
            &mut target_tree,
            &decrypt_options(&pack, &decrypted),
        )
        .unwrap();

        assert_eq!(plan.stats.encrypted, 0);
        assert!(
            !plan
                .actions
                .iter()
                .any(|a| matches!(a, Action::DecryptFile { .. }))
        );
        assert!(
            !plan
                .actions
                .iter()
                .any(|a| matches!(a, Action::CreateDir(_)))
        );
        assert_eq!(
            link_src(&plan, &target.join("plain.txt")),
            pack.join("plain.txt")
        );
    }

    #[test]
    fn plan_install_decrypt_folds_plain_dirs_only() {
        let dir = tempfile::TempDir::with_prefix("stow-cm-test-").unwrap();
        let pack = dir.path().join("pack");
        let target = dir.path().join("target");
        let decrypted = dir.path().join("decrypted");
        std::fs::create_dir_all(pack.join("plain_dir")).unwrap();
        std::fs::create_dir_all(pack.join("secret_dir")).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(pack.join("plain_dir/a.txt"), "plain a\n").unwrap();
        std::fs::write(pack.join("plain_dir/b.txt"), "plain b\n").unwrap();
        std::fs::write(pack.join("secret_dir/c.txt"), "x &{secret} y\n").unwrap();

        let mut pack_tree = vtree::VNode::scan(&pack, false).unwrap();
        let mut target_tree = vtree::VNode::scan_guided(&target, &pack_tree, false).unwrap();
        let plan = plan_install(
            &mut pack_tree,
            &mut target_tree,
            &decrypt_options(&pack, &decrypted),
        )
        .unwrap();

        // 纯普通文件目录折叠为单个目录链接
        assert_eq!(
            link_src(&plan, &target.join("plain_dir")),
            pack.join("plain_dir")
        );
        // 含占位符目录不折叠，逐文件解密
        assert!(!has_create_link_to(&plan, &target.join("secret_dir")));
        assert_eq!(
            link_src(&plan, &target.join("secret_dir/c.txt")),
            decrypted.join("secret_dir/c.txt")
        );
        assert_eq!(plan.stats.encrypted, 1);
    }

    #[test]
    fn plan_install_fold_ignores_placeholder_when_encryption_disabled() {
        let dir = tempfile::TempDir::with_prefix("stow-cm-test-").unwrap();
        let pack = dir.path().join("pack");
        let target = dir.path().join("target");
        std::fs::create_dir_all(pack.join("sub")).unwrap();
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(pack.join("sub/secret.txt"), "x &{secret} y\n").unwrap();

        let mut pack_tree = vtree::VNode::scan(&pack, false).unwrap();
        let mut target_tree = vtree::VNode::scan_guided(&target, &pack_tree, false).unwrap();
        // 未开启加密配置：占位符只是普通文本，目录仍应折叠
        let plan = plan_install(&mut pack_tree, &mut target_tree, &fold_options()).unwrap();

        assert_eq!(link_src(&plan, &target.join("sub")), pack.join("sub"));
        assert_eq!(plan.stats.encrypted, 0);
    }
}
