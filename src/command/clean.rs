use std::convert::identity;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::anyhow;
use log::{info, warn};

use super::{pack_envs, resolve_track_file};
use crate::config::Config;
use crate::error::Result;
use crate::executor;
use crate::planner;
use crate::util;

/// clean packages
pub fn clean<P: AsRef<Path>>(config: &Arc<Config>, pack: P, dry_run: bool) -> Result<()> {
    let pack = Arc::new(pack.as_ref().to_path_buf());
    let pack_name = config.resolve_pack_name(&pack)?.into_owned();
    info!("cleaning");

    clean_link(config, &pack, dry_run)?;

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

/// clean links via filesystem scan: find all symlinks under target pointing to pack
fn clean_link(config: &Arc<Config>, pack: &Arc<PathBuf>, dry_run: bool) -> Result<()> {
    let pack_name = config.resolve_pack_name(pack.as_ref())?.into_owned();
    let Some(target) = config.target.as_ref() else {
        warn!("target is none, skip clean links");
        return Ok(());
    };

    let track_file = resolve_track_file(pack)?;

    // ── 扫描文件系统，找到所有指向 pack 的符号链接 ──
    let symlinks = util::find_prefix_symlink(target, pack.as_ref())?;

    let encrypted = config
        .encrypted
        .as_ref()
        .is_some_and(|it| it.enable.is_some_and(identity));
    let decrypted_path = if encrypted {
        Some(
            config
                .encrypted
                .as_ref()
                .and_then(|it| it.decrypted_path.as_ref())
                .ok_or_else(|| anyhow!("{pack_name}: decrypted path is not configured"))?
                .as_path(),
        )
    } else {
        None
    };

    let state_dir = track_file.parent().map(std::path::Path::to_path_buf);
    let plan = planner::plan_clean(&symlinks, decrypted_path, state_dir.as_deref());

    executor::execute_plan(&plan, dry_run)?;

    Ok(())
}
