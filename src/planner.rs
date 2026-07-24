//! 计划引擎（Planner）模块。
//!
//! 对虚拟文件树进行差异比较（diff），生成 `ActionPlan`，
//! 是虚拟树管线（virtual tree pipeline）中的"调和（reconcile）"阶段。
//!
//! 提供四个核心入口：
//! - [`plan_install`] — 安装计划（pack → target）
//! - [`plan_remove`]  — 移除计划（track → target）
//! - [`plan_reload`]  — 重载计划（remove + install 合并去重）
//! - [`plan_clean`]   — 清理计划（从文件系统扫描结果构建 RemoveLink 计划）

use std::path::{Path, PathBuf};

use regex::RegexSet;

use crate::action::{Action, ActionPlan, PlanStats};
use crate::error::Result;
use crate::symlink::{Symlink, SymlinkMode};
use crate::track_file::Track;
use crate::util;
use crate::vtree::{VNode, VNodeKind};

// ── 公共入口 ──

/// 解密选项，由命令模块从 Config 中提取并传给 planner
#[derive(Debug, Clone)]
pub struct DecryptOption {
    pub decrypted_path: PathBuf,
    pub key: Vec<u8>,
    pub alg: String,
    pub left_boundary: String,
    pub right_boundary: String,
    pub pack_path: PathBuf,
}

/// track file 写入元信息，由命令模块传递给 planner 以生成 `WriteTrackFile` 动作
#[derive(Debug, Clone)]
pub struct TrackWriteInfo {
    /// track file 目标路径
    pub track_file: PathBuf,
    /// pack 名称
    pub pack_name: String,
    /// pack 原始路径
    pub pack_path: PathBuf,
    /// 安装目标目录
    pub target: PathBuf,
    /// 安装时的 symlink 模式
    pub symlink_mode: Option<SymlinkMode>,
    /// 是否启用了加解密
    pub encrypted: bool,
}

/// 合并选项，控制树合并时的行为
#[derive(Debug)]
pub struct MergeOption {
    /// 忽略匹配规则（不安装匹配的文件）
    pub ignore: Option<RegexSet>,
    /// 覆盖匹配规则（覆盖已存在的文件）
    pub over: Option<RegexSet>,
    /// 是否启用目录折叠（fold）
    pub fold: Option<bool>,
    /// 符号链接模式（symlink 或 copy）
    pub symlink_mode: Option<SymlinkMode>,
}

/// 计划选项，包含树合并配置、解密配置和可选的 track file 写入配置
#[derive(Debug)]
pub struct PlanOption {
    /// 树合并选项
    pub merge: MergeOption,
    /// 解密选项（None 表示不启用加密/解密）
    pub decrypt: Option<DecryptOption>,
    /// track file 写入选项（None 表示不写入 track file）
    pub track_write: Option<TrackWriteInfo>,
}

/// 为 `pack_tree` 生成安装计划。
///
/// 递归比较 `pack_tree` 与 `target_tree`，为每个 pack 叶子节点生成
/// `CreateLink` 操作。遇到冲突时生成 `Conflict`，匹配忽略规则时跳过，
/// 满足覆盖规则时强制覆盖。支持目录折叠（fold）优化。
///
/// 如果 `options.decrypt` 已设置，生成的计划中会在 `CreateLink` 之前
/// 插入 `DecryptFile` 操作，并将 `CreateLink.src` 重写为解密后的路径。
///
/// 如果 `track_write` 已设置，会额外生成：
/// - `CreateDir(decrypted_path)`（解密场景下）
/// - `WriteTrackFile` 写入安装后的 track 记录
pub fn plan_install(
    pack_tree: &VNode,
    target_tree: &VNode,
    options: &PlanOption,
) -> Result<ActionPlan> {
    let target_base = target_tree.abs_path.clone();
    let children_plan = install_children(
        &pack_tree.children,
        &target_tree.children,
        &target_base,
        options,
    );
    let mut plan = ActionPlan {
        actions: children_plan.actions,
        stats: *children_plan.stats,
    };

    // 注入解密动作
    if let Some(decrypt) = &options.decrypt {
        // 创建解密目标目录
        plan.actions
            .insert(0, Action::CreateDir(decrypt.decrypted_path.clone()));
        plan.stats.dirs_to_create += 1;

        let mut decrypt_actions = Vec::new();
        for action in &mut plan.actions {
            if let Action::CreateLink { src, .. } = action {
                let decrypted_file_path =
                    util::change_base_path(&*src, &decrypt.pack_path, &decrypt.decrypted_path)?;
                decrypt_actions.push(Action::DecryptFile {
                    src: src.clone(),
                    to: decrypted_file_path.clone(),
                    key: decrypt.key.clone(),
                    alg: decrypt.alg.clone(),
                    left_boundary: decrypt.left_boundary.clone(),
                    right_boundary: decrypt.right_boundary.clone(),
                });
                *src = decrypted_file_path;
            }
        }
        plan.stats.encrypted = decrypt_actions.len();
        // 将 DecryptFile 插入到 CreateDir 之后、CreateLink 之前
        plan.actions.splice(1..1, decrypt_actions);
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
                decrypted_path: options
                    .decrypt
                    .as_ref()
                    .map(|d| d.decrypted_path.clone()),
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
                target_tree.remove(rel);
            } else {
                plan.stats.conflicts += 1;
                plan.actions.push(Action::Conflict {
                    dst: link.dst.clone(),
                    reason: consistency_failure_reason(link, node),
                });
            }
        }
        // else: 虚拟树中找不到此路径（可能已被手动从文件系统删除）
    }

    // 先清理解密目录（可能不在 pack state home 下）
    if let Some(path) = &track.decrypted_path {
        plan.actions.push(Action::RemoveDir { path: path.clone() });
        plan.stats.dirs_removed += 1;
    }

    // 再删除整个 pack state 目录（$XDG_STATE_HOME/stow-cm/{PACK_ID}/）
    // 放在最后确保 decrypted_path 内部子目录也一并被 remove_dir_all 兜底清理
    if let Some(dir) = state_dir {
        plan.actions.push(Action::RemoveDir {
            path: dir.to_path_buf(),
        });
        plan.stats.dirs_removed += 1;
    }

    plan
}

/// 判断 track 中的链接记录与文件系统实际节点是否一致。
fn is_consistent(link: &crate::symlink::Symlink, node: &VNode) -> bool {
    match (&link.mode, &node.kind) {
        (SymlinkMode::Symlink, VNodeKind::Symlink { target }) => target == &link.src,
        (SymlinkMode::Copy, VNodeKind::File) => true,
        _ => false,
    }
}

/// 生成一致性检查失败的原因描述。
fn consistency_failure_reason(link: &crate::symlink::Symlink, node: &VNode) -> String {
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
        (SymlinkMode::Symlink, VNodeKind::Dir) => {
            format!(
                "expected symlink at '{}', found directory",
                link.dst.display()
            )
        }
        (SymlinkMode::Copy, VNodeKind::Symlink { .. }) => {
            format!(
                "expected regular file at '{}', found symlink",
                link.dst.display()
            )
        }
        (SymlinkMode::Copy, VNodeKind::Dir) => {
            format!(
                "expected regular file at '{}', found directory",
                link.dst.display()
            )
        }
        // 此分支不会执行（(Copy, File) 在 is_consistent 中返回 true），仅为满足穷尽匹配
        (SymlinkMode::Copy, VNodeKind::File) => {
            format!("unexpected inconsistency at '{}'", link.dst.display())
        }
    }
}

/// 生成重载计划：先移除、再安装，合并并去重。
///
/// `remove_target` 从 `track.target` 扫描而来（会被 `plan_remove` 修改），
/// `install_target` 从 `config.target` 扫描而来。若为 `None` 表示与 `remove_target`
/// 同一棵树——install 阶段使用 remove 清理后的版本，避免已移除节点产生误报冲突。
///
/// 合并规则：
/// - 仅当 track 与 config 的 symlink mode **都为** Symlink 时才去重；
///   copy 模式不做去重，remove 和 install 各自独立执行。
/// - 若同一 `dst` 在移除和安装计划中同时出现，且 `src` 相同 → 同时删除此二操作（无净变更）
/// - 若 `src` 不同 → 两者都保留
/// - 去重仅影响 `RemoveLink`/`CreateLink` 对；解密目录的 `RemoveDir`/`CreateDir` 不受影响
pub fn plan_reload(
    pack_tree: &VNode,
    remove_target: &mut VNode,
    install_target: Option<&VNode>,
    track: &Track,
    options: &PlanOption,
) -> Result<ActionPlan> {
    let remove_plan = plan_remove(track, remove_target, None);

    // install_target 为 None 时表示与 remove 同一棵树 → 使用清理后的版本
    let install_base = install_target.unwrap_or(remove_target);

    // decrypt + track_write 统一由 plan_install 注入，plan_reload 只负责合并
    let install_plan = plan_install(pack_tree, install_base, options)?;

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

/// 生成清理计划。
///
/// 将文件系统扫描得到的符号链接列表转换为 `ActionPlan`，
/// 每条链接生成一个 `RemoveLink` 操作。
/// 若指定了 `decrypted_path` 则追加 `RemoveDir` 操作，
/// 若指定了 `state_dir` 则追加 `RemoveDir` 清理 pack state 目录。
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
        });
        plan.stats.dirs_removed += 1;
    }

    if let Some(dir) = state_dir {
        plan.actions.push(Action::RemoveDir {
            path: dir.to_path_buf(),
        });
        plan.stats.dirs_removed += 1;
    }

    plan
}

// ── 内部类型 ──

/// 递归安装子节点时返回的结果，含该子树的操作与统计。
struct ChildrenPlan {
    /// 产生的操作列表。
    actions: Vec<Action>,
    /// 该子树内部的统计信息（含子孙节点）。
    stats: Box<PlanStats>,
    /// 所有子孙是否均可折叠（无冲突、无忽略）。
    foldable: bool,
    /// 是否存在被忽略的子孙节点。
    had_ignored: bool,
}

impl ChildrenPlan {
    fn empty() -> Self {
        Self {
            actions: Vec::new(),
            stats: Box::default(),
            foldable: true,
            had_ignored: false,
        }
    }
}

// ── 递归安装核心 ──

/// 递归处理 pack 的一组子节点，将每个子节点安装到 target 对应位置。
///
/// `target_children` 是目标树中当前目录的子节点列表，
/// `target_base` 是当前目标目录的绝对路径。
fn install_children(
    pack_children: &[VNode],
    target_children: &[VNode],
    target_base: &Path,
    options: &PlanOption,
) -> ChildrenPlan {
    let mut result = ChildrenPlan::empty();

    for pack_child in pack_children {
        // 在当前层级的目标子节点中查找同名节点
        let target_child = target_children
            .iter()
            .find(|tc| tc.rel_path == pack_child.rel_path);

        let child_target_base = target_base.join(&pack_child.rel_path);
        let child_plan = install_node(pack_child, target_child, &child_target_base, options);

        // 合并统计信息
        result.stats.links_to_create += child_plan.stats.links_to_create;
        result.stats.conflicts += child_plan.stats.conflicts;
        result.stats.ignored += child_plan.stats.ignored;
        result.stats.overridden += child_plan.stats.overridden;
        result.stats.encrypted += child_plan.stats.encrypted;

        if child_plan.had_ignored {
            result.had_ignored = true;
        }
        if !child_plan.foldable {
            result.foldable = false;
        }

        result.actions.extend(child_plan.actions);
    }

    result
}

/// 递归处理单个 pack 节点，返回其安装计划。
///
/// - `target_child` — 目标树中对应的节点（如果存在）
/// - `target_dst`  — 该 pack 节点在目标文件系统中的预期绝对路径
fn install_node(
    pack: &VNode,
    target_child: Option<&VNode>,
    target_dst: &Path,
    options: &PlanOption,
) -> ChildrenPlan {
    // ── 忽略检查 ──
    if let Some(ignore_re) = &options.merge.ignore {
        if ignore_re.is_match(&pack.abs_path.to_string_lossy()) {
            let mut stats = Box::<PlanStats>::default();
            stats.ignored = 1;
            return ChildrenPlan {
                actions: Vec::new(),
                stats,
                foldable: false,
                had_ignored: true,
            };
        }
    }

    match &pack.kind {
        VNodeKind::File | VNodeKind::Symlink { .. } => {
            plan_leaf(pack, target_child, target_dst, options)
        }
        VNodeKind::Dir => plan_dir(pack, target_child, target_dst, options),
    }
}

/// 处理叶子节点（File 或 Symlink）。
fn plan_leaf(
    pack: &VNode,
    target_child: Option<&VNode>,
    target_dst: &Path,
    options: &PlanOption,
) -> ChildrenPlan {
    let mode = options.merge.symlink_mode.clone().unwrap_or_default();

    if let Some(target_node) = target_child {
        // 目标已存在 — 检查是否可覆盖
        if let Some(over_re) = &options.merge.over {
            if over_re.is_match(&pack.abs_path.to_string_lossy()) {
                let mut stats = Box::<PlanStats>::default();
                stats.links_to_create = 1;
                stats.overridden = 1;
                return ChildrenPlan {
                    actions: vec![Action::CreateLink {
                        src: pack.abs_path.clone(),
                        dst: target_node.abs_path.clone(),
                        mode,
                    }],
                    stats,
                    foldable: true,
                    had_ignored: false,
                };
            }
        }
        // 存在且不可覆盖 → 冲突，不可折叠
        let mut stats = Box::<PlanStats>::default();
        stats.conflicts = 1;
        ChildrenPlan {
            actions: vec![Action::Conflict {
                dst: target_node.abs_path.clone(),
                reason: "file already exists".to_string(),
            }],
            stats,
            foldable: false,
            had_ignored: false,
        }
    } else {
        // 目标不存在 → 正常创建链接
        let mut stats = Box::<PlanStats>::default();
        stats.links_to_create = 1;
        ChildrenPlan {
            actions: vec![Action::CreateLink {
                src: pack.abs_path.clone(),
                dst: target_dst.to_path_buf(),
                mode,
            }],
            stats,
            foldable: true,
            had_ignored: false,
        }
    }
}

/// 处理目录节点：递归子节点，检查折叠条件。
fn plan_dir(
    pack: &VNode,
    target_child: Option<&VNode>,
    target_dst: &Path,
    options: &PlanOption,
) -> ChildrenPlan {
    let mode = options.merge.symlink_mode.clone().unwrap_or_default();
    let fold_enabled = options.merge.fold.unwrap_or(false);

    // 递归处理所有子节点
    let target_children = target_child.map_or(&[] as &[VNode], |tc| tc.children.as_slice());

    let children_plan = install_children(&pack.children, target_children, target_dst, options);

    // 检查 target 中是否存在 pack 没有的子节点
    let has_new_sub = target_children
        .iter()
        .any(|tc| !pack.children.iter().any(|pc| pc.rel_path == tc.rel_path));

    let should_fold =
        fold_enabled && children_plan.foldable && !children_plan.had_ignored && !has_new_sub;

    if should_fold {
        // 用目标节点 abs_path（如果存在）或计算出的 target_dst 作为 dst
        let dst = target_child.map_or_else(|| target_dst.to_path_buf(), |tc| tc.abs_path.clone());

        let mut stats = Box::<PlanStats>::default();
        stats.links_to_create = 1;

        ChildrenPlan {
            actions: vec![Action::CreateLink {
                src: pack.abs_path.clone(),
                dst,
                mode,
            }],
            stats,
            foldable: true,
            had_ignored: false,
        }
    } else {
        children_plan
    }
}

// ── 计划合并与去重 ──

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
            if let Action::RemoveLink { dst, src, .. } = a {
                if let Some(install_src) = install_index.get(dst) {
                    if install_src == src {
                        return Some((dst.clone(), src.clone()));
                    }
                }
            }
            None
        })
        .collect();

    // 从 install_plan 中移除匹配的 CreateLink
    install_plan.actions.retain(|a| {
        if let Action::CreateLink { dst, src, .. } = a {
            if to_remove_from_install
                .iter()
                .any(|(d, s)| d == dst && s == src)
            {
                install_plan.stats.links_to_create =
                    install_plan.stats.links_to_create.saturating_sub(1);
                return false;
            }
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
        {
            if to_remove_from_install
                .iter()
                .any(|(d, s)| d == dst && s == src)
            {
                remove_count = remove_count.saturating_sub(1);
                continue;
            }
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
    };
    merged_actions.extend(install_plan.actions);
    ActionPlan {
        actions: merged_actions,
        stats: combined_stats,
    }
}

// ── 测试 ──

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::symlink::{Symlink, SymlinkMode};

    // ── 测试辅助 ──

    /// 创建一个 File 类型的 VNode。
    fn file_node(name: &str, abs: &str) -> VNode {
        VNode {
            rel_path: PathBuf::from(name),
            abs_path: PathBuf::from(abs),
            kind: VNodeKind::File,
            children: Vec::new(),
        }
    }

    /// 创建一个 Dir 类型的 VNode（带子节点）。
    fn dir_node(name: &str, abs: &str, children: Vec<VNode>) -> VNode {
        VNode {
            rel_path: PathBuf::from(name),
            abs_path: PathBuf::from(abs),
            kind: VNodeKind::Dir,
            children,
        }
    }

    /// 创建一个 Symlink 类型的 VNode。
    fn symlink_node(name: &str, abs: &str, link_target: &str) -> VNode {
        VNode {
            rel_path: PathBuf::from(name),
            abs_path: PathBuf::from(abs),
            kind: VNodeKind::Symlink {
                target: PathBuf::from(link_target),
            },
            children: Vec::new(),
        }
    }

    /// 创建空的 PlanOption（不含解密配置）。
    fn default_options() -> PlanOption {
        PlanOption {
            merge: MergeOption {
                ignore: None,
                over: None,
                fold: None,
                symlink_mode: None,
            },
            decrypt: None,
            track_write: None,
        }
    }

    /// 创建只启用 fold 的 PlanOption（不含解密配置）。
    fn fold_options() -> PlanOption {
        PlanOption {
            merge: MergeOption {
                ignore: None,
                over: None,
                fold: Some(true),
                symlink_mode: None,
            },
            decrypt: None,
            track_write: None,
        }
    }

    /// 创建一个简单的 Track（含一条 Symlink）。
    fn make_track(src: &str, dst: &str) -> Track {
        Track {
            links: vec![Symlink {
                src: PathBuf::from(src),
                dst: PathBuf::from(dst),
                mode: SymlinkMode::Symlink,
            }],
            decrypted_path: None,
            encrypted: false,
            pack_name: None,
            pack_path: None,
            target: None,
            symlink_mode: None,
        }
    }

    // ── plan_install 测试 ──

    #[test]
    fn plan_install_basic_file() {
        // pack: /pack/file.txt  →  target: (empty)
        let pack = dir_node("", "/pack", vec![file_node("file.txt", "/pack/file.txt")]);
        let target = dir_node("", "/target", Vec::new());

        let plan = plan_install(&pack, &target, &default_options()).unwrap();

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
        let pack = dir_node(
            "",
            "/pack",
            vec![
                file_node("a.txt", "/pack/a.txt"),
                file_node("b.txt", "/pack/b.txt"),
            ],
        );
        let target = dir_node("", "/target", Vec::new());

        let plan = plan_install(&pack, &target, &default_options()).unwrap();

        assert_eq!(plan.stats.links_to_create, 2);
        assert_eq!(plan.actions.len(), 2);
    }

    #[test]
    fn plan_install_conflict() {
        // target 中已存在同名文件 → 冲突
        let pack = dir_node("", "/pack", vec![file_node("file.txt", "/pack/file.txt")]);
        let target = dir_node(
            "",
            "/target",
            vec![file_node("file.txt", "/target/file.txt")],
        );

        let plan = plan_install(&pack, &target, &default_options()).unwrap();

        assert_eq!(plan.stats.conflicts, 1);
        assert_eq!(plan.stats.links_to_create, 0);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(plan.actions[0], Action::Conflict { .. }));
    }

    #[test]
    fn plan_install_override() {
        // over 规则匹配 → 强制覆盖
        let pack = dir_node("", "/pack", vec![file_node("file.txt", "/pack/file.txt")]);
        let target = dir_node(
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

        let plan = plan_install(&pack, &target, &options).unwrap();

        assert_eq!(plan.stats.overridden, 1);
        assert_eq!(plan.stats.conflicts, 0);
        assert_eq!(plan.stats.links_to_create, 1);
        assert!(matches!(plan.actions[0], Action::CreateLink { .. }));
    }

    #[test]
    fn plan_install_ignore() {
        // ignore 规则匹配 → 跳过
        let pack = dir_node(
            "",
            "/pack",
            vec![
                file_node("readme.md", "/pack/readme.md"),
                file_node("config.txt", "/pack/config.txt"),
            ],
        );
        let target = dir_node("", "/target", Vec::new());

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

        let plan = plan_install(&pack, &target, &options).unwrap();

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
        let pack = dir_node(
            "",
            "/pack",
            vec![dir_node(
                "subdir",
                "/pack/subdir",
                vec![file_node("inner.txt", "/pack/subdir/inner.txt")],
            )],
        );
        let target = dir_node("", "/target", Vec::new());

        let plan = plan_install(&pack, &target, &default_options()).unwrap();

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
        let pack = dir_node(
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
        let target = dir_node("", "/target", Vec::new());

        let plan = plan_install(&pack, &target, &fold_options()).unwrap();

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
        let pack = dir_node(
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
        let target = dir_node("", "/target", Vec::new());

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

        let plan = plan_install(&pack, &target, &options).unwrap();

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
        let pack = dir_node(
            "",
            "/pack",
            vec![dir_node(
                "sub",
                "/pack/sub",
                vec![file_node("x.txt", "/pack/sub/x.txt")],
            )],
        );
        let target = dir_node(
            "",
            "/target",
            vec![dir_node(
                "sub",
                "/target/sub",
                vec![file_node("extra.txt", "/target/sub/extra.txt")],
            )],
        );

        let plan = plan_install(&pack, &target, &fold_options()).unwrap();

        // 不可折叠：target 中有 extra.txt
        assert_eq!(plan.actions.len(), 1);
        if let Action::CreateLink { ref src, .. } = plan.actions[0] {
            assert_eq!(src, &PathBuf::from("/pack/sub/x.txt"));
        }
    }

    #[test]
    fn plan_install_symlink_node() {
        // Symlink 类型的节点应被视为叶子，生成 CreateLink
        let pack = dir_node(
            "",
            "/pack",
            vec![symlink_node("link.txt", "/pack/link.txt", "/etc/somefile")],
        );
        let target = dir_node("", "/target", Vec::new());

        let plan = plan_install(&pack, &target, &default_options()).unwrap();

        assert_eq!(plan.stats.links_to_create, 1);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(plan.actions[0], Action::CreateLink { .. }));
    }

    // ── plan_remove 测试 ──

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

    // ── plan_reload 测试 ──

    #[test]
    fn plan_reload_dedup_same_src() {
        // 同一 dst 在 remove 和 install 中 src 相同 → 抵消
        let pack = dir_node("", "/pack", vec![file_node("file.txt", "/pack/file.txt")]);
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

        let plan = plan_reload(&pack, &mut target, None, &track, &default_options()).unwrap();

        // 同一路径同时移除和创建，src 相同，互相抵消
        assert_eq!(plan.stats.links_to_create, 0);
        assert_eq!(plan.stats.links_to_remove, 0);
        assert_eq!(plan.actions.len(), 0);
    }

    #[test]
    fn plan_reload_different_src() {
        // 同一 dst，但 src 不同（文件位置变更）→ 两者都保留
        let pack = dir_node(
            "",
            "/pack",
            vec![file_node("file.txt", "/pack/new/file.txt")],
        );
        // target_tree 包含目标文件（Symlink 节点指向旧的 src），plan_remove 会检测到 drift 冲突
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

        let plan = plan_reload(&pack, &mut target, None, &track, &default_options()).unwrap();

        // src 不同 → remove 和 create 都应保留
        assert_eq!(plan.stats.links_to_create, 1);
        assert_eq!(plan.stats.links_to_remove, 1);
        assert_eq!(plan.actions.len(), 2);
    }

    #[test]
    fn plan_reload_new_file() {
        // pack 中有新文件，track 为空
        let pack = dir_node(
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

        let plan = plan_reload(&pack, &mut target, None, &track, &default_options()).unwrap();

        assert_eq!(plan.stats.links_to_create, 1);
        assert_eq!(plan.stats.links_to_remove, 0);
        assert_eq!(plan.actions.len(), 1);
    }

    #[test]
    fn plan_reload_removed_file() {
        // pack 中删除了文件（track 有记录但 pack 没有）
        let pack = dir_node("", "/pack", Vec::new());
        let mut target = dir_node(
            "",
            "/target",
            vec![symlink_node("old.txt", "/target/old.txt", "/pack/old.txt")],
        );
        let track = make_track("/pack/old.txt", "/target/old.txt");

        let plan = plan_reload(&pack, &mut target, None, &track, &default_options()).unwrap();

        // 只有 remove，没有 create
        assert_eq!(plan.stats.links_to_create, 0);
        assert_eq!(plan.stats.links_to_remove, 1);
        assert_eq!(plan.actions.len(), 1);
        assert!(matches!(plan.actions[0], Action::RemoveLink { .. }));
    }

    // ── plan_clean 测试 ──

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

    // ── merge_and_dedup 测试 ──

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
