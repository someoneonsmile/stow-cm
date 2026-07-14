use log::debug;
use serde::Serialize;
use tokio::fs;

use crate::error::Result;
use crate::paths::stow_cm_state_dir;
use crate::symlink::SymlinkMode;
use crate::track_file::Track;

/// list 输出的单行记录（同时用于 JSON 序列化）
#[derive(Serialize)]
struct PackEntry {
    pack: String,
    target: String,
    links: usize,
    mode: String,
    encrypted: bool,
}

/// 扫描 `$XDG_STATE_HOME/stow-cm/` 下所有 track file，输出已安装 pack 列表
pub async fn list(json: bool) -> Result<()> {
    let state_dir = stow_cm_state_dir();

    if !state_dir.try_exists()? {
        println!("No installed packs found.");
        return Ok(());
    }

    let mut entries: Vec<PackEntry> = Vec::new();
    let mut dir_reader = fs::read_dir(&state_dir).await?;
    while let Some(entry) = dir_reader.next_entry().await? {
        let entry_path = entry.path();
        if !entry_path.is_dir() {
            continue;
        }
        let track_path = entry_path.join("track.toml");
        if !track_path.try_exists()? {
            continue;
        }
        let pack_id = entry_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        let content = match fs::read_to_string(&track_path).await {
            Ok(c) => c,
            Err(e) => {
                debug!("Failed to read track file {}: {e}", track_path.display());
                continue;
            }
        };
        let track = match toml::from_str::<Track>(&content) {
            Ok(t) => t,
            Err(e) => {
                debug!("Failed to parse track file {}: {e}", track_path.display());
                continue;
            }
        };

        let pack_name = track.pack_name.unwrap_or_else(|| {
            track
                .pack_path
                .as_ref()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .map_or_else(|| pack_id.to_string(), String::from)
        });

        let target = track
            .target
            .as_ref()
            .map(|t| t.display().to_string())
            .or_else(|| {
                track
                    .links
                    .first()
                    .and_then(|s| s.dst.parent().map(|p| p.display().to_string()))
            })
            .unwrap_or_else(|| "-".to_string());

        let mode = if track.links.iter().any(|l| l.mode == SymlinkMode::Copy) {
            "copy"
        } else {
            "symlink"
        };

        entries.push(PackEntry {
            pack: pack_name,
            target,
            links: track.links.len(),
            mode: mode.to_string(),
            encrypted: track.decrypted_path.is_some(),
        });
    }

    if entries.is_empty() {
        println!("No installed packs found.");
        return Ok(());
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        print_table(&entries);
    }

    Ok(())
}

fn print_table(entries: &[PackEntry]) {
    let max_pack = entries
        .iter()
        .map(|e| e.pack.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let max_target = entries
        .iter()
        .map(|e| e.target.len())
        .max()
        .unwrap_or(6)
        .max(6);
    let max_links = entries
        .iter()
        .map(|e| e.links.to_string().len())
        .max()
        .unwrap_or(5)
        .max(5);

    println!(
        "{:<pack_w$}  {:<target_w$}  {:>links_w$}  {:<8}  ENCRYPTED",
        "PACK",
        "TARGET",
        "LINKS",
        "MODE",
        pack_w = max_pack,
        target_w = max_target,
        links_w = max_links,
    );

    for entry in entries {
        println!(
            "{:<pack_w$}  {:<target_w$}  {:>links_w$}  {:<8}  {}",
            entry.pack,
            entry.target,
            entry.links,
            entry.mode,
            if entry.encrypted { "yes" } else { "no" },
            pack_w = max_pack,
            target_w = max_target,
            links_w = max_links,
        );
    }
}
