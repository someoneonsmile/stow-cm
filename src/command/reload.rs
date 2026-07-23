use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use log::{info, warn};

use super::{pack_envs, resolve_track_file};
use crate::config::Config;
use crate::error::Result;
use crate::executor;
use crate::planner;
use crate::planner::MergeOption;
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

    let track_file = resolve_track_file(pack, &pack_name)?;

    // ── 读取旧的 track file ──
    let old_track = if track_file.try_exists()? {
        let content = std::fs::read_to_string(&track_file)?;
        Some(toml::from_str::<Track>(&content)?)
    } else {
        None
    };

    let ignore_re = config.ignore_regex()?;
    let over_re = config.over_regex()?;

    // ── 一次扫描两棵虚拟树 ──
    let pack_tree = vtree::VNode::scan(pack.as_ref(), false)?;
    let target_tree = vtree::VNode::scan(target, false)?;

    let options = MergeOption {
        ignore: ignore_re,
        over: over_re,
        fold: config.fold,
        symlink_mode: config.symlink_mode.clone(),
    };

    // ── 制定重载计划 ──
    let empty_track = Track {
        links: Vec::new(),
        decrypted_path: None,
        pack_name: None,
        pack_path: None,
        target: None,
    };
    let track = old_track.as_ref().unwrap_or(&empty_track);
    let plan = planner::plan_reload(&pack_tree, &target_tree, track, &options);

    if plan.has_conflicts() {
        let conflicts: Vec<_> = plan
            .actions
            .iter()
            .filter_map(|a| match a {
                crate::action::Action::Conflict { dst, reason } => {
                    Some(format!("  {} ({})", dst.display(), reason))
                }
                _ => None,
            })
            .collect();
        if !conflicts.is_empty() {
            anyhow::bail!("check conflict:\n{}", conflicts.join("\n"));
        }
    }

    // ── 执行计划 ──
    executor::execute_plan(&plan, dry_run)?;

    // ── 写入新的 track file ──
    let symlinks: Vec<crate::symlink::Symlink> = plan
        .actions
        .iter()
        .filter_map(|a| match a {
            crate::action::Action::CreateLink { src, dst, mode } => Some(crate::symlink::Symlink {
                src: src.clone(),
                dst: dst.clone(),
                mode: mode.clone(),
            }),
            _ => None,
        })
        .collect();

    if dry_run {
        info!("would write track file: {}", track_file.display());
    } else {
        std::fs::create_dir_all(track_file.parent().with_context(|| {
            format!(
                "{pack_name}: failed to find track file parent, {}",
                track_file.display()
            )
        })?)?;

        std::fs::write(
            &track_file,
            toml::to_string_pretty(&Track {
                decrypted_path: old_track.as_ref().and_then(|t| t.decrypted_path.clone()),
                links: symlinks,
                pack_name: Some(pack_name.clone()),
                pack_path: Some((**pack).clone()),
                target: Some(target.clone()),
            })?,
        )?;
    }

    Ok(())
}
