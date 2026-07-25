use std::convert::identity;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::anyhow;
use log::{info, warn};

use super::{pack_envs, resolve_track_file};
use crate::config::{Config, EncryptedParams};
use crate::error::Result;
use crate::executor;
use crate::planner;
use crate::planner::{MergeOption, PlanOption, TrackWriteInfo};
use crate::track_file::Track;
use crate::vtree;

/// reload packages — 一次扫描，先 remove 后 install，合并为单个 `ActionPlan`
pub fn reload(config: &Arc<Config>, pack: impl AsRef<Path>, dry_run: bool) -> Result<()> {
    let pack = Arc::new(pack.as_ref().to_path_buf());
    let pack_name = config.resolve_pack_name(&pack)?.into_owned();
    info!("reloading");

    reload_link(config, &pack, dry_run)?;

    // execute the clear script (remove old)
    if let Some(command) = &config.clear {
        if dry_run {
            info!("would run clear script (dry-run)");
        } else {
            info!("running clear script");
            command.execute(&*pack, pack_envs(&pack, &pack_name))?;
            info!("clear script done");
        }
    }

    // execute the init script (install new)
    if let Some(command) = &config.init {
        if dry_run {
            info!("would run init script (dry-run)");
        } else {
            info!("running init script");
            command.execute(&*pack, pack_envs(&pack, &pack_name))?;
            info!("init script done");
        }
    }

    Ok(())
}

fn reload_link(config: &Arc<Config>, pack: &Arc<PathBuf>, dry_run: bool) -> Result<()> {
    let pack_name = config.resolve_pack_name(pack.as_ref())?.into_owned();
    let Some(target) = config.target.as_ref() else {
        warn!("target is none, skip reload links");
        return Ok(());
    };

    let track_file = resolve_track_file(pack)?;

    // ── 读取旧的 track file ──
    let old_track = if track_file.try_exists()? {
        let content = std::fs::read_to_string(&track_file)?;
        Some(toml::from_str::<Track>(&content)?)
    } else {
        None
    };

    let ignore_re = config.ignore_regex()?;
    let over_re = config.over_regex()?;

    // ── 扫描 pack 树（pack 内容）──
    let pack_tree = vtree::VNode::scan(pack.as_ref(), false)?;

    // ── 分别构建目标树（同路径则共享一棵，避免 clone）──
    let mut install_target_tree = vtree::VNode::scan(target, false)?;

    let remove_target_path = old_track
        .as_ref()
        .and_then(|t| t.target.as_deref())
        .unwrap_or(target);

    let mut options = PlanOption {
        merge: MergeOption {
            ignore: ignore_re,
            over: over_re,
            fold: config.fold,
            symlink_mode: config.symlink_mode.clone(),
        },
        decrypt: None,
        track_write: None,
    };

    let encrypted_enabled = config
        .encrypted
        .as_ref()
        .is_some_and(|it| it.enable.is_some_and(identity));
    let decrypted_path_opt = config
        .encrypted
        .as_ref()
        .and_then(|it| it.decrypted_path.as_ref());

    if encrypted_enabled {
        let decrypted_path = decrypted_path_opt
            .ok_or_else(|| anyhow!("{pack_name}: decrypted path is not configured"))?;

        let params = config
            .encrypted
            .as_ref()
            .ok_or_else(|| anyhow!("{pack_name}: encrypted config not found"))?
            .resolve(&pack_name)?;
        let EncryptedParams {
            key,
            left_boundary,
            right_boundary,
            encrypted_alg,
        } = params;

        options.decrypt = Some(planner::DecryptOption {
            decrypted_path: decrypted_path.clone(),
            key: key.clone(),
            alg: encrypted_alg.to_string(),
            left_boundary: left_boundary.to_string(),
            right_boundary: right_boundary.to_string(),
            pack_path: (**pack).clone(),
        });
    }

    options.track_write = Some(TrackWriteInfo {
        track_file: track_file.clone(),
        pack_name: pack_name.clone(),
        pack_path: (**pack).clone(),
        target: target.clone(),
        symlink_mode: config.symlink_mode.clone(),
        encrypted: encrypted_enabled,
    });

    let plan = if let Some(ref track) = old_track {
        if remove_target_path == target.as_path() {
            planner::plan_reload(
                &pack_tree,
                &mut install_target_tree,
                None,
                track,
                &options,
            )?
        } else {
            let mut remove_target_tree = vtree::VNode::scan(remove_target_path, false)?;
            planner::plan_reload(
                &pack_tree,
                &mut remove_target_tree,
                Some(&install_target_tree),
                track,
                &options,
            )?
        }
    } else {
        warn!("no previous installation found, reload will proceed as a fresh install");
        planner::plan_install(&pack_tree, &install_target_tree, &options)?
    };

    // ── 执行计划 ──
    executor::execute_plan(&plan, dry_run)?;

    Ok(())
}
