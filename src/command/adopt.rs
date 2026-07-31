use std::convert::identity;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, bail};
use log::info;

use super::{pack_envs, resolve_track_file};
use crate::command::init::write_default_config;
use crate::config::{Config, EncryptedParams};
use crate::constants::CONFIG_FILE_NAME;
use crate::error::Result;
use crate::executor;
use crate::planner::{self, MergeOption, PlanOption, TrackWriteInfo};
use crate::symlink::SymlinkMode;
use crate::vtree;

pub fn adopt(global: &Config, source: &Path, to: &Path, dry_run: bool) -> Result<()> {
    let dir_name = source
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| anyhow!("{}: cannot determine pack name", source.display()))?;
    let pack = to.join(dir_name);
    let config_path = pack.join(CONFIG_FILE_NAME);

    if pack.exists() && !config_path.exists() {
        let mut entries = fs::read_dir(&pack)?;
        if entries.next().is_some() {
            bail!(
                "pack directory '{}' already exists with content \
                 but no {CONFIG_FILE_NAME} — refusing to adopt.\n\
                 Remove the directory or create a {CONFIG_FILE_NAME} first.",
                pack.display()
            );
        }
    }

    let config = Arc::new(Config::for_pack(&pack, global, None, true)?);
    let pack_name = config.resolve_pack_name(&pack)?.into_owned();
    let target: PathBuf = if config_path.exists() {
        let cfg_target = config
            .target
            .as_ref()
            .ok_or_else(|| anyhow!("{pack_name}: target is not configured"))?;
        let tc = fs::canonicalize(cfg_target);
        let sc = fs::canonicalize(source);
        if !matches!((&tc, &sc), (Ok(tc), Ok(sc)) if tc == sc) {
            bail!(
                "target in {CONFIG_FILE_NAME} does not match source '{}'",
                source.display()
            );
        }
        cfg_target.clone()
    } else {
        source.to_path_buf()
    };

    info!("adopting");

    let track_file = resolve_track_file(&pack)?;
    if track_file.try_exists()? {
        bail!("{pack_name}: pack has been install")
    }

    let ignore_re = config.ignore_regex()?;

    let mut target_tree = vtree::VNode::scan(&target, false)?;
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

    if !config_path.exists() {
        if dry_run {
            info!("would generate stow-cm.toml for {pack_name} (dry-run)");
        } else {
            write_default_config(&config_path, global, &pack, &pack_name, Some(&target))?;
            info!("generated stow-cm.toml for {pack_name}");
        }
    }

    let over_re = config.over_regex()?;
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
            pack_path: pack.clone(),
        });
    }

    options.track_write = Some(TrackWriteInfo {
        track_file,
        pack_name: pack_name.clone(),
        pack_path: pack.clone(),
        target: target.clone(),
        symlink_mode: config.symlink_mode.clone(),
        encrypted: decrypted,
    });

    let install_plan = planner::plan_install(&mut pack_tree, &mut target_tree, &options)?;
    executor::execute_plan(&install_plan, dry_run)?;

    if let Some(command) = &config.init {
        if dry_run {
            info!("would run init script (dry-run)");
        } else {
            info!("running init script");
            command.execute(&pack, pack_envs(&pack, &pack_name))?;
            info!("init script done");
        }
    }

    Ok(())
}
