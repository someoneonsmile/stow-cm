use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;

use log::info;

use crate::action::{Action, ActionPlan};
use crate::config::Config;
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
pub fn execute_plan(plan: &ActionPlan, dry_run: bool) -> Result<()> {
    info!("{plan}");
    if dry_run {
        return Ok(());
    }

    for action in &plan.actions {
        execute_action(action)?;
    }
    Ok(())
}

fn execute_action(action: &Action) -> Result<()> {
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
            .map_err(|e| anyhow::anyhow!("无法创建目录 {}: {e}", path.display())),
        Action::Conflict { dst, reason } => {
            log::warn!("冲突: {} ({})", dst.display(), reason);
            Ok(()) // 冲突不阻断执行，仅警告
        }
        Action::DecryptFile { src, to } => {
            // 读取源文件，解密，写入目标位置
            let content = std::fs::read_to_string(src)
                .map_err(|e| anyhow::anyhow!("无法读取文件 {}: {e}", src.display()))?;

            // 解密功能后续由 crypto 模块集成。
            // 目前 install 流程中解密操作在执行前已完成，此处直接复制。
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| anyhow::anyhow!("无法创建解密目录: {e}"))?;
            }
            std::fs::write(to, content)
                .map_err(|e| anyhow::anyhow!("无法写入解密文件 {}: {e}", to.display()))
        }
    }
}
