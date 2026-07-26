use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;

use log::{debug, info};

use crate::action::{Action, ActionPlan};
use crate::config::Config;
use crate::crypto;
use crate::error::Result;
use crate::symlink::Symlink;
use crate::util;

pub fn exec_all<F, P>(common_config: &Arc<Option<Config>>, packs: Vec<P>, f: F) -> Result<()>
where
    F: Fn(&Arc<Config>, P) -> Result<()>,
    P: AsRef<Path>,
{
    let global = common_config
        .deref()
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("global config not loaded"))?;
    let mut errors = Vec::new();
    for pack in packs {
        let config = match Config::for_pack(pack.as_ref(), global, None, false) {
            Ok(c) => c,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        let pack_name = config.resolve_pack_name(pack.as_ref())?.into_owned();
        info!("========== {pack_name} ==========");
        let result = util::scoped_log_prefix(&pack_name, || f(&Arc::new(config), pack));
        if let Err(e) = result {
            errors.push(e);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "{} pack(s) failed:\n{}",
            errors.len(),
            errors
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        ))
    }
}

/// 执行 `ActionPlan` 中的操作
///
/// `dry_run` 为 true 时只打印计划不执行文件操作。
/// 执行前检查冲突：若存在未被 ignore/override 覆盖的冲突，终止执行并报告给用户。
pub fn execute_plan(plan: &ActionPlan, dry_run: bool) -> Result<()> {
    info!("plan:\n{plan}");

    // 冲突检查：必须在任何实际文件操作前终止
    if plan.has_conflicts() {
        let mut details = Vec::new();
        for action in &plan.actions {
            if let Action::Conflict { dst, reason } = action {
                details.push(format!("  {} ({})", dst.display(), reason));
            }
        }
        anyhow::bail!(
            "{} conflict(s) detected — resolve before executing:\n{}",
            details.len(),
            details.join("\n")
        );
    }

    if dry_run {
        return Ok(());
    }

    for action in &plan.actions {
        execute_action(action)?;
    }
    Ok(())
}

fn execute_action(action: &Action) -> Result<()> {
    debug!("execute_action: {action}");
    match action {
        Action::CreateLink { src, dst, mode } => {
            let symlink = Symlink {
                src: src.clone(),
                dst: dst.clone(),
                mode: mode.clone(),
            };
            symlink.create(true)
        }
        Action::RemoveLink { src, dst, mode } => {
            let symlink = Symlink {
                src: src.clone(),
                dst: dst.clone(),
                mode: mode.clone(),
            };
            symlink.remove()
        }
        Action::CreateDir(path) => std::fs::create_dir_all(path)
            .map_err(|e| anyhow::anyhow!("Failed to create directory {}: {e}", path.display())),
        Action::Conflict { dst, reason } => {
            anyhow::bail!(
                "Unexpected conflict reached execution phase: {} ({})",
                dst.display(),
                reason
            )
        }
        Action::DecryptFile {
            src,
            to,
            key,
            alg,
            left_boundary,
            right_boundary,
        } => {
            let content = std::fs::read_to_string(src).map_err(|e| {
                anyhow::anyhow!("Failed to read file for decryption {}: {e}", src.display())
            })?;

            let decrypted =
                crypto::decrypt_inline(&content, alg, key, left_boundary, right_boundary, true)?;

            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    anyhow::anyhow!("Failed to create decrypt target directory: {e}")
                })?;
            }
            std::fs::write(to, decrypted).map_err(|e| {
                anyhow::anyhow!("Failed to write decrypted file {}: {e}", to.display())
            })
        }
        Action::RemoveDir { path, .. } => {
            if path.try_exists()? {
                std::fs::remove_dir_all(path).map_err(|e| {
                    anyhow::anyhow!("Failed to clean decrypted dir {}: {e}", path.display())
                })
            } else {
                Ok(())
            }
        }
        Action::RemoveFile { path, .. } => {
            if path.try_exists()? {
                std::fs::remove_file(path)
                    .map_err(|e| anyhow::anyhow!("Failed to remove file {}: {e}", path.display()))
            } else {
                Ok(())
            }
        }
        Action::WriteTrackFile { path, track } => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to create track file parent {}: {e}",
                        parent.display()
                    )
                })?;
            }
            let content = toml::to_string_pretty(track).map_err(|e| {
                anyhow::anyhow!("Failed to serialize track file: {e}")
            })?;
            std::fs::write(path, &content).map_err(|e| {
                anyhow::anyhow!("Failed to write track file {}: {e}", path.display())
            })
        }
    }
}
