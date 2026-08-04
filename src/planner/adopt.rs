//! 接管（adopt）计划生成子模块。
//!
//! 包含 [`plan_adopt`]，复用安装计划逻辑实现反向接管。

use crate::action::{ActionPlan, PlanStats};
use crate::error::Result;

use super::{PlanOption, VNode};

/// 为 adopt（反向接管）生成文件移动计划。
///
/// 复用 [`super::install::plan_install`] 对比 `source_tree` 与 `pack_tree` 做
/// ignore 过滤和 fold，使用 `SymlinkMode::Move` 将文件从 source 移动到 pack。
/// 若 pack 中已存在同名文件，会作为冲突暴露给用户处理。
///
/// `source` 路径从 `source_tree.abs_path` 获取。
pub fn plan_adopt(
    source_tree: &mut VNode,
    pack_tree: &mut VNode,
    options: &PlanOption,
) -> Result<ActionPlan> {
    let install_plan = super::install::plan_install(source_tree, pack_tree, options)?;

    Ok(ActionPlan {
        stats: PlanStats {
            links_to_create: install_plan.stats.links_to_create,
            conflicts: install_plan.stats.conflicts,
            ignored: install_plan.stats.ignored,
            ..PlanStats::default()
        },
        actions: install_plan.actions,
    })
}
