use std::fs;
use std::ops::Deref;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::sync::Arc;

use crate::action::{Action, ActionPlan, RemoveDirMode};
use crate::config::Config;
use crate::crypto;
use crate::error::Result;
use crate::symlink::Symlink;
use crate::util;
use anyhow::{anyhow, bail};
use binaryornot::is_binary;
use log::{debug, info, warn};

// TODO: 等 RFC 3955 (Named Fn trait parameters) 稳定后，
// 可以在闭包签名中直接用 `dry_run: bool` 命名参数替代此类型别名。
// RFC: https://github.com/rust-lang/rfcs/pull/3955
// 追踪: https://github.com/rust-lang/rust/issues/158499
pub type DryRun = bool;

pub fn exec_all<F, P>(
    common_config: &Arc<Option<Config>>,
    packs: Vec<P>,
    dry_run: DryRun,
    f: F,
) -> Result<()>
where
    F: Fn(&Arc<Config>, P, DryRun) -> Result<()>,
    P: AsRef<Path>,
{
    let global = common_config
        .deref()
        .as_ref()
        .ok_or_else(|| anyhow!("global config not loaded"))?;
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
        if dry_run {
            info!("[dry-run] ========== {pack_name} ==========");
        } else {
            info!("========== {pack_name} ==========");
        }
        let result = util::scoped_log_prefix(&pack_name, || f(&Arc::new(config), pack, dry_run));
        if let Err(e) = result {
            errors.push(e);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(
            "{} pack(s) failed:\n{}",
            errors.len(),
            errors
                .iter()
                .map(ToString::to_string)
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
        bail!(
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
        Action::CreateDir(path) => fs::create_dir_all(path)
            .map_err(|e| anyhow!("Failed to create directory {}: {e}", path.display())),
        Action::Conflict { dst, reason } => {
            bail!(
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
            // 二进制文件跳过加解密，创建从解密路径到原文件的软链接（与 crypto_process 行为一致）
            if is_binary(src).unwrap_or(true) {
                warn!(
                    "{} is binary file, symlinking without decryption",
                    src.display()
                );
                if let Some(parent) = to.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|e| anyhow!("Failed to create decrypt target directory: {e}"))?;
                }
                return symlink(src, to).map_err(|e| {
                    anyhow!(
                        "Failed to symlink binary file {} -> {}: {e}",
                        src.display(),
                        to.display()
                    )
                });
            }

            let content = fs::read_to_string(src).map_err(|e| {
                anyhow!("Failed to read file for decryption {}: {e}", src.display())
            })?;

            let decrypted =
                crypto::decrypt_inline(&content, alg, key, left_boundary, right_boundary, true)?;

            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| anyhow!("Failed to create decrypt target directory: {e}"))?;
            }
            fs::write(to, decrypted)
                .map_err(|e| anyhow!("Failed to write decrypted file {}: {e}", to.display()))
        }
        Action::RemoveDir { path, mode, .. } => {
            if !path.try_exists()? {
                return Ok(());
            }
            match mode {
                RemoveDirMode::All => fs::remove_dir_all(path)
                    .map_err(|e| anyhow!("Failed to remove dir {}: {e}", path.display())),
                // 计划层只给出最顶层空目录（嵌套空目录靠递归一并清理），
                // 因此这里允许递归，但前提是整棵子树不含任何文件/链接。
                RemoveDirMode::IfEmpty => {
                    if contains_only_dirs(path)? {
                        fs::remove_dir_all(path).map_err(|e| {
                            anyhow!("Failed to remove empty dir {}: {e}", path.display())
                        })
                    } else {
                        // 虚拟树判定为空、磁盘上却有文件/链接：计划与磁盘不一致，
                        // 属于不变量被打破。绝不删除内容，同时报错暴露问题。
                        bail!(
                            "refusing to remove non-empty directory {}: expected it to contain only empty directories, found files or links — nothing was deleted",
                            path.display()
                        );
                    }
                }
            }
        }
        Action::RemoveFile { path, .. } => {
            if path.try_exists()? {
                fs::remove_file(path)
                    .map_err(|e| anyhow!("Failed to remove file {}: {e}", path.display()))
            } else {
                Ok(())
            }
        }
        Action::WriteTrackFile { path, track } => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| {
                    anyhow!(
                        "Failed to create track file parent {}: {e}",
                        parent.display()
                    )
                })?;
            }
            let content = toml::to_string_pretty(track)
                .map_err(|e| anyhow!("Failed to serialize track file: {e}"))?;
            fs::write(path, &content)
                .map_err(|e| anyhow!("Failed to write track file {}: {e}", path.display()))
        }
    }
}

/// 判断 `path` 这棵子树是否只由目录组成（不含普通文件、符号链接等）。
///
/// 用于 [`RemoveDirMode::IfEmpty`]：计划层只列出最顶层空目录，嵌套空目录
/// 需要递归清理；但递归删除前必须先确认整棵子树里没有文件，避免虚拟树
/// 与磁盘不一致时误删内容。
fn contains_only_dirs(path: &Path) -> Result<bool> {
    for entry in
        fs::read_dir(path).map_err(|e| anyhow!("Failed to read dir {}: {e}", path.display()))?
    {
        let entry = entry.map_err(|e| anyhow!("Failed to read dir entry: {e}"))?;
        let entry_path = entry.path();
        // 用 symlink_metadata，避免跟随符号链接到目录
        let meta = fs::symlink_metadata(&entry_path)
            .map_err(|e| anyhow!("Failed to read metadata of {}: {e}", entry_path.display()))?;
        if !meta.is_dir() || !contains_only_dirs(&entry_path)? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remove_dir(path: &Path, mode: RemoveDirMode) -> Result<()> {
        execute_action(&Action::RemoveDir {
            path: path.to_path_buf(),
            reason: "test".to_string(),
            mode,
        })
    }

    #[test]
    fn remove_dir_if_empty_errors_on_non_empty() {
        let dir = tempfile::TempDir::with_prefix("stow-cm-test-").unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("keep.txt"), "x").unwrap();

        // 计划与磁盘不一致 → 报错暴露问题，同时绝不删除内容
        assert!(remove_dir(&sub, RemoveDirMode::IfEmpty).is_err());
        assert!(sub.join("keep.txt").exists());
    }

    #[test]
    fn remove_dir_if_empty_removes_empty() {
        let dir = tempfile::TempDir::with_prefix("stow-cm-test-").unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();

        remove_dir(&sub, RemoveDirMode::IfEmpty).unwrap();

        assert!(!sub.exists());
    }

    #[test]
    fn remove_dir_if_empty_removes_nested_empty() {
        // 计划层只给最顶层空目录，嵌套空目录需要递归清理
        let dir = tempfile::TempDir::with_prefix("stow-cm-test-").unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir_all(sub.join("nested/deeper")).unwrap();

        remove_dir(&sub, RemoveDirMode::IfEmpty).unwrap();

        assert!(!sub.exists());
    }

    #[test]
    fn remove_dir_if_empty_errors_on_nested_non_empty() {
        let dir = tempfile::TempDir::with_prefix("stow-cm-test-").unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir_all(sub.join("nested")).unwrap();
        fs::write(sub.join("nested/keep.txt"), "x").unwrap();

        assert!(remove_dir(&sub, RemoveDirMode::IfEmpty).is_err());
        assert!(sub.join("nested/keep.txt").exists());
    }

    #[test]
    fn remove_dir_if_empty_errors_on_symlink() {
        // 目录里只有一条符号链接也算“非空”，不能递归删除
        let dir = tempfile::TempDir::with_prefix("stow-cm-test-").unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        symlink(dir.path().join("missing"), sub.join("link")).unwrap();

        assert!(remove_dir(&sub, RemoveDirMode::IfEmpty).is_err());

        // 悬空链接用 exists() 会因跟随目标而返回 false，这里用 symlink_metadata
        assert!(fs::symlink_metadata(sub.join("link")).is_ok());
    }

    #[test]
    fn remove_dir_all_removes_non_empty() {
        let dir = tempfile::TempDir::with_prefix("stow-cm-test-").unwrap();
        let sub = dir.path().join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join("gone.txt"), "x").unwrap();

        remove_dir(&sub, RemoveDirMode::All).unwrap();

        assert!(!sub.exists());
    }
}
