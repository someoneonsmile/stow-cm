mod adopt;
mod clean;
mod crypto;
mod init;
mod install;
mod list;
mod remove;
mod status;

pub use adopt::adopt;
pub use clean::clean;
pub use crypto::{decrypt, encrypt};
pub use init::init;
pub use install::install;
pub use list::list;
pub use remove::remove;
pub use status::status;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::anyhow;
use maplit::hashmap;

use crate::config::Config;
use crate::constants::{PACK_ID_ENV, PACK_NAME_ENV, TRACK_FILE_NAME};
use crate::error::Result;
use crate::paths::{pack_track_file, stow_cm_state_dir};
use crate::track_file::Track;
use crate::util;

/// 构造 pack 操作的环境变量 `[(PACK_ID_ENV, hash), (PACK_NAME_ENV, pack_name)]`，
/// 消除 `install`/`clean`/`remove` 中的重复注入逻辑。
pub(super) fn pack_envs(pack: &Path, pack_name: &str) -> [(&'static str, String); 2] {
    [
        (PACK_ID_ENV, util::hash(&pack.to_string_lossy())),
        (PACK_NAME_ENV, pack_name.to_owned()),
    ]
}

/// 解析 pack 对应的 track file 路径，消除 `install`/`clean`/`remove` 中的重复逻辑。
pub(super) fn resolve_track_file(pack: &Path, pack_name: &str) -> Result<PathBuf> {
    let context_map = hashmap! {
        PACK_ID_ENV => util::hash(&pack.to_string_lossy()),
        PACK_NAME_ENV => pack_name.to_owned(),
    };
    let track_file =
        util::shell_expand_full_with_context(pack_track_file(), |key| context_map.get(key))?;
    Ok(track_file)
}

/// 根据 `PACK_ID`（支持前缀匹配）查找对应的 pack 路径。
///
/// 扫描 `$XDG_STATE_HOME/stow-cm/` 下所有 track file，
/// 将输入的 ID 与目录名（`PACK_ID`）进行前缀匹配：
/// - 精确匹配 1 个 → 返回对应的 `pack_path`
/// - 匹配 0 个   → 报错 "not found"
/// - 匹配 ≥2 个  → 报错 "ambiguous"，列出所有候选项
pub fn resolve_pack_ids(ids: &[String]) -> Result<Vec<PathBuf>> {
    let state_dir = stow_cm_state_dir();
    if !state_dir.exists() {
        return Err(anyhow!(
            "no installed packs found (state directory missing). Use `stow-cm list` to check."
        ));
    }

    let mut installed: Vec<(String, PathBuf)> = Vec::new();
    for entry in std::fs::read_dir(&state_dir)? {
        let entry = entry?;
        let id_dir = entry.path();
        if !id_dir.is_dir() {
            continue;
        }
        let track_path = id_dir.join(TRACK_FILE_NAME);
        if !track_path.exists() {
            continue;
        }
        let Some(pack_id) = id_dir
            .file_name()
            .and_then(|n| n.to_str())
            .map(String::from)
        else {
            continue;
        };
        let Ok(content) = std::fs::read_to_string(&track_path) else {
            continue;
        };
        let track: Track = match toml::from_str(&content) {
            Ok(t) => t,
            Err(_) => continue,
        };
        let Some(pack_path) = track.pack_path else {
            continue;
        };
        installed.push((pack_id, pack_path));
    }

    if installed.is_empty() {
        return Err(anyhow!(
            "no installed packs found. Use `stow-cm list` to check."
        ));
    }

    let mut results = Vec::new();
    for id in ids {
        let matches: Vec<_> = installed
            .iter()
            .filter(|(pid, _)| pid.starts_with(id.as_str()))
            .collect();

        match matches.len() {
            0 => {
                return Err(anyhow!(
                    "PACK_ID \"{id}\" not found. Use `stow-cm list` to see installed packs."
                ));
            }
            1 => {
                let Some((_, pack_path)) = matches.first() else {
                    continue;
                };
                results.push(pack_path.clone());
            }
            _ => {
                let candidates: Vec<String> = matches
                    .iter()
                    .map(|(pid, pp)| format!("  {pid}  →  {}", pp.display()))
                    .collect();
                return Err(anyhow!(
                    "PACK_ID prefix \"{id}\" matches {} packs:\n{}\nUse a longer prefix.",
                    matches.len(),
                    candidates.join("\n")
                ));
            }
        }
    }

    Ok(results)
}

/// reload packages
pub fn reload(config: &Arc<Config>, pack: impl AsRef<Path>) -> Result<()> {
    remove::remove(config, &pack)?;
    install::install(config, &pack)?;
    Ok(())
}
