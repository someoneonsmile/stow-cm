use std::path::Path;
use std::sync::Arc;

use anyhow::anyhow;
use log::info;

use crate::config::Config;
use crate::error::Result;
use crate::executor;
use crate::planner::{self, MergeOption, PlanOption};
use crate::symlink::SymlinkMode;
use crate::vtree;

pub fn adopt(config: &Arc<Config>, pack: impl AsRef<Path>, dry_run: bool) -> Result<()> {
    info!("adopting");

    let pack = pack.as_ref().to_path_buf();
    let pack_name = config.resolve_pack_name(&pack)?.into_owned();
    let ignore_re = config.ignore_regex()?;
    let target = config
        .target
        .as_ref()
        .ok_or_else(|| anyhow!("{pack_name}: target is not configured"))?;

    let mut target_tree = vtree::VNode::scan(target, false)?;
    if target_tree.children.is_empty() {
        info!("source directory is empty, nothing to adopt");
        return Ok(());
    }
    let mut pack_tree = vtree::VNode::scan(&pack, false)?;

    let adopt_options = PlanOption {
        merge: MergeOption {
            ignore: ignore_re.clone(),
            over: None,
            fold: Some(true),
            symlink_mode: Some(SymlinkMode::Move),
        },
        decrypt: None,
        track_write: None,
    };
    let move_plan = planner::plan_adopt(&mut target_tree, &mut pack_tree, &adopt_options)?;
    executor::execute_plan(&move_plan, dry_run)?;

    let mut pack_tree = vtree::VNode::scan(&pack, false)?;
    let mut target_tree = vtree::VNode::scan(target, false)?;
    let install_options = PlanOption {
        merge: MergeOption {
            ignore: ignore_re,
            over: None,
            fold: config.fold,
            symlink_mode: config.symlink_mode.clone(),
        },
        decrypt: None,
        track_write: None,
    };
    let install_plan = planner::plan_install(&mut pack_tree, &mut target_tree, &install_options)?;
    executor::execute_plan(&install_plan, dry_run)?;

    Ok(())
}
