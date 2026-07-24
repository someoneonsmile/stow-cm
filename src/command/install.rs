use std::convert::identity;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, bail};
use log::{info, warn};

use super::{pack_envs, resolve_track_file};
use crate::config::{Config, EncryptedParams};
use crate::error::Result;
use crate::executor;
use crate::planner;
use crate::planner::{MergeOption, PlanOption, TrackWriteInfo};
use crate::vtree;

/// install packages
pub fn install(config: &Arc<Config>, pack: impl AsRef<Path>, dry_run: bool) -> Result<()> {
    let pack = Arc::new(pack.as_ref().to_path_buf());
    let pack_name = config.resolve_pack_name(&pack)?.into_owned();
    info!("installing");

    install_link(config, &pack, dry_run)?;

    // execute the init script
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

/// install link
fn install_link(config: &Arc<Config>, pack: &Arc<PathBuf>, dry_run: bool) -> Result<()> {
    let pack_name = config.resolve_pack_name(pack.as_ref())?.into_owned();
    let Some(target) = config.target.as_ref() else {
        warn!("target is none, skip install links");
        return Ok(());
    };

    // if track file already exists, then the pack has been installed
    let track_file = resolve_track_file(pack)?;
    if track_file.try_exists()? {
        bail!("{pack_name}: pack has been install")
    }

    let ignore_re = config.ignore_regex()?;
    let over_re = config.over_regex()?;

    // ── Virtual tree pipeline: scan → plan → execute ──
    let pack_tree = vtree::VNode::scan(pack.as_ref(), false)?;
    let target_tree = vtree::VNode::scan(target, false)?;

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

    let decrypted = config
        .encrypted
        .as_ref()
        .is_some_and(|it| it.enable.is_some_and(identity));
    let decrypted_path = config
        .encrypted
        .as_ref()
        .and_then(|it| it.decrypted_path.as_ref());

    if decrypted {
        let decrypted_path = decrypted_path
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
        encrypted: decrypted,
    });

    let plan = planner::plan_install(&pack_tree, &target_tree, &options)?;

    // ── Execute ──
    executor::execute_plan(&plan, dry_run)?;

    Ok(())
}
