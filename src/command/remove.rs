use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::anyhow;
use log::{info, warn};

use super::{pack_envs, resolve_track_file};
use crate::config::Config;
use crate::error::Result;
use crate::executor;
use crate::planner;
use crate::track_file::Track;
use crate::vtree;

/// remove packages
pub fn remove<P: AsRef<Path>>(config: &Arc<Config>, pack: P, dry_run: bool) -> Result<()> {
    let pack = Arc::new(pack.as_ref().to_path_buf());
    let pack_name = config.resolve_pack_name(&pack)?.into_owned();
    info!("removing");

    remove_link(config, &pack, dry_run)?;

    // execute the clear script
    if let Some(command) = &config.clear {
        if dry_run {
            info!("would run clear script (dry-run)");
        } else {
            info!("running clear script");
            command.execute(&*pack, pack_envs(&pack, &pack_name))?;
            info!("clear script done");
        }
    }

    Ok(())
}

/// remove links
fn remove_link(config: &Config, pack: &Arc<PathBuf>, dry_run: bool) -> Result<()> {
    let track_file = resolve_track_file(pack)?;

    if !track_file.try_exists()? {
        warn!("no links installed");
        return Ok(());
    }

    let track: Track = toml::from_str(&std::fs::read_to_string(track_file.as_path())?)?;

    // 优先使用 track 中记录的 target（安装时记录），降级使用 config.target
    let target = track
        .target
        .as_deref()
        .or(config.target.as_deref())
        .ok_or_else(|| {
            anyhow!(
                "Cannot determine target: neither track file nor config contains target directory"
            )
        })?;

    // ── 扫描目标目录虚拟树，生成移除计划 ──
    let mut target_tree = vtree::VNode::scan(target, false)?;
    let state_dir = track_file.parent().map(std::path::Path::to_path_buf);
    let plan = planner::plan_remove(&track, &mut target_tree, state_dir.as_deref());

    executor::execute_plan(&plan, dry_run)?;

    Ok(())
}
