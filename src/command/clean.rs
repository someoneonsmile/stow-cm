use std::convert::identity;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::anyhow;
use log::{debug, info, warn};

use super::{pack_envs, resolve_track_file};
use crate::action::{Action, ActionPlan, PlanStats};
use crate::config::Config;
use crate::error::Result;
use crate::executor;
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
    debug!("clean paths: {symlinks:?}");

    if !symlinks.is_empty() {
        let actions: Vec<Action> = symlinks
            .iter()
            .map(|s| Action::RemoveLink {
                src: s.src.clone(),
                dst: s.dst.clone(),
                mode: s.mode.clone(),
            })
            .collect();
        let plan = ActionPlan {
            stats: PlanStats {
                links_to_remove: actions.len(),
                ..PlanStats::default()
            },
            actions,
        };
        executor::execute_plan(&plan, dry_run)?;
    }

    // 清理解密目录（dry-run 时跳过文件删除）
    let encrypted = config
        .encrypted
        .as_ref()
        .is_some_and(|it| it.enable.is_some_and(identity));
    if encrypted && !dry_run {
        let decrypted_path = config
            .encrypted
            .as_ref()
            .and_then(|it| it.decrypted_path.as_ref())
            .ok_or_else(|| anyhow!("{pack_name}: decrypted path is not configured"))?;
        if decrypted_path.try_exists()? {
            info!("clean decrypted dir, {}", decrypted_path.display());
            std::fs::remove_dir_all(decrypted_path)?;
        }
    }

    // 删除 track 文件（dry-run 时跳过）
    if !dry_run && track_file.try_exists()? {
        debug!("clean track file, {}", track_file.display());
        std::fs::remove_file(track_file)?;
    }

    Ok(())
}
