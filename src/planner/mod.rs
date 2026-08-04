//! 计划引擎（Planner）模块。
//!
//! 对虚拟文件树进行差异比较（diff），生成 `ActionPlan`，
//! 是虚拟树管线（virtual tree pipeline）中的"调和（reconcile）"阶段。
//!
//! 提供四个核心入口：
//! - [`plan_install`] — 安装计划（pack → target）
//! - [`plan_remove`]  — 移除计划（track → target）
//! - [`plan_reload`]  — 重载计划（remove + install 合并去重）
//! - [`plan_clean`]   — 清理计划（从文件系统扫描结果构建 `RemoveLink` 计划）
//! - [`plan_adopt`]   — 接管计划（source → pack 移动 + 安装链接）

use std::path::PathBuf;

use regex::RegexSet;

use crate::action::{Action, PlanStats};
#[cfg(test)]
use crate::symlink::Symlink;
use crate::symlink::SymlinkMode;
#[cfg(test)]
use crate::track_file::Track;
use crate::vtree::{VNode, VNodeKind};

// ── 子模块声明 ──

pub mod adopt;
pub mod clean;
pub mod install;
pub mod reload;
pub mod remove;

// ── Re-export 公共入口 ──

pub use adopt::plan_adopt;
pub use clean::plan_clean;
pub use install::plan_install;
pub use reload::plan_reload;
pub use remove::plan_remove;

// ── 公共类型 ──

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

// ── 测试辅助函数（跨子模块共享） ──

#[cfg(test)]
#[allow(unused)]
fn file_node(name: &str, abs: &str) -> VNode {
    VNode {
        rel_path: PathBuf::from(name),
        abs_path: PathBuf::from(abs),
        kind: VNodeKind::File,
        children: Vec::new(),
    }
}

#[cfg(test)]
#[allow(unused)]
fn dir_node(name: &str, abs: &str, children: Vec<VNode>) -> VNode {
    VNode {
        rel_path: PathBuf::from(name),
        abs_path: PathBuf::from(abs),
        kind: VNodeKind::Dir,
        children,
    }
}

#[cfg(test)]
#[allow(unused)]
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

#[cfg(test)]
#[allow(unused)]
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

#[cfg(test)]
#[allow(unused)]
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

#[cfg(test)]
#[allow(unused)]
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
