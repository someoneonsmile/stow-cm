use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use log::{info, warn};
use serde::Serialize;
use tokio::fs;

use crate::command::resolve_track_file;
use crate::config::Config;
use crate::constants::TRACK_FILE_NAME;
use crate::error::Result;
use crate::paths::stow_cm_state_dir;
use crate::symlink::{Symlink, SymlinkMode};
use crate::track_file::Track;
use crate::util;

/// 链接状态枚举，按严重程度升序排列（OK < MISSING/DANGLING < OVERWRITTEN/DRIFT）
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
enum LinkStatus {
    Ok,
    Missing,
    Dangling,
    Overwritten,
    Drift,
}

impl LinkStatus {
    fn icon(self) -> &'static str {
        match self {
            LinkStatus::Ok => "OK",
            LinkStatus::Missing => "MI",
            LinkStatus::Dangling => "DA",
            LinkStatus::Overwritten => "OW",
            LinkStatus::Drift => "DR",
        }
    }
}

/// 单个链接的状态记录（JSON 序列化用）
#[derive(Debug, Serialize)]
struct LinkEntry {
    pack: String,
    status: LinkStatus,
    src: String,
    dst: String,
    mode: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    fixed: bool,
}

/// 检查单个链接的实际状态
async fn check_symlink(link: &Symlink) -> LinkStatus {
    let metadata = fs::symlink_metadata(&link.dst).await;
    match metadata {
        Err(_) => LinkStatus::Missing,
        Ok(meta) => match link.mode {
            SymlinkMode::Symlink => {
                if !meta.file_type().is_symlink() {
                    return LinkStatus::Overwritten;
                }
                let Ok(target) = fs::read_link(&link.dst).await else {
                    return LinkStatus::Missing;
                };
                if target != link.src {
                    return LinkStatus::Drift;
                }
                match fs::symlink_metadata(&link.src).await {
                    Ok(_) => LinkStatus::Ok,
                    Err(_) => LinkStatus::Dangling,
                }
            }
            SymlinkMode::Copy => {
                if !meta.file_type().is_file() && !meta.file_type().is_symlink() {
                    return LinkStatus::Overwritten;
                }
                match fs::metadata(&link.src).await {
                    Ok(_) => LinkStatus::Ok,
                    Err(_) => LinkStatus::Dangling,
                }
            }
        },
    }
}

/// 尝试修复缺失的链接
async fn fix_missing(link: &Symlink) -> Result<()> {
    info!(
        "fixing missing: {} -> {}",
        link.dst.display(),
        link.src.display()
    );
    link.create(true).await
}

/// 读取 track.toml 并检查所有链接状态
async fn check_pack_links(pack_name: &str, track: &Track, fix: bool) -> Result<Vec<LinkEntry>> {
    let mut entries = Vec::new();

    for link in &track.links {
        let status = check_symlink(link).await;
        let mut fixed = false;

        if status == LinkStatus::Missing && fix {
            if let Err(e) = fix_missing(link).await {
                warn!("failed to fix {}: {e}", link.dst.display());
            } else {
                fixed = true;
            }
        }

        entries.push(LinkEntry {
            pack: pack_name.to_string(),
            status: if fixed { LinkStatus::Ok } else { status },
            src: link.src.display().to_string(),
            dst: link.dst.display().to_string(),
            mode: format!("{:?}", link.mode).to_lowercase(),
            fixed,
        });
    }

    Ok(entries)
}

/// 从 track.toml 路径解析 pack 名称和 Track 记录
async fn read_track_from_path(track_path: &Path) -> Option<(String, Track)> {
    let content = fs::read_to_string(track_path).await.ok()?;
    let track: Track = toml::from_str(&content).ok()?;
    let pack_name = track.pack_name.clone().unwrap_or_else(|| {
        track
            .pack_path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .map_or_else(
                || {
                    track_path
                        .parent()
                        .and_then(|p| p.file_name())
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                        .to_string()
                },
                String::from,
            )
    });
    Some((pack_name, track))
}

/// 扫描 `state_dir` 下所有已安装 pack
async fn status_all(fix: bool, json: bool) -> Result<()> {
    let state_dir = stow_cm_state_dir();

    if !state_dir.try_exists()? {
        println!("No installed packs found.");
        return Ok(());
    }

    let mut all_entries: Vec<LinkEntry> = Vec::new();
    let mut dir_reader = fs::read_dir(&state_dir).await?;
    while let Some(entry) = dir_reader.next_entry().await? {
        let entry_path = entry.path();
        if !entry_path.is_dir() {
            continue;
        }
        let track_path = entry_path.join(TRACK_FILE_NAME);
        if !track_path.try_exists()? {
            continue;
        }
        let Some((pack_name, track)) = read_track_from_path(&track_path).await else {
            continue;
        };
        let entries = check_pack_links(&pack_name, &track, fix).await?;
        all_entries.extend(entries);
    }

    if all_entries.is_empty() {
        println!("No installed packs found.");
        return Ok(());
    }

    output_entries(&all_entries, json);
    Ok(())
}

/// 检查指定 pack 路径的状态
async fn status_packs(
    global_config: &Config,
    paths: Vec<PathBuf>,
    fix: bool,
    json: bool,
) -> Result<()> {
    let paths = util::canonicalize(paths).await?;
    let mut all_entries: Vec<LinkEntry> = Vec::new();

    for pack in &paths {
        let config = Config::for_pack(pack, global_config, None, false)?;
        let pack_name = config.resolve_pack_name(pack)?.into_owned();
        let track_file = resolve_track_file(pack, &pack_name)?;

        if !track_file.try_exists()? {
            info!("no track file for pack: {pack_name}, skipping");
            continue;
        }

        let content = fs::read_to_string(&track_file).await?;
        let track: Track = toml::from_str(&content)?;
        let entries = check_pack_links(&pack_name, &track, fix).await?;
        all_entries.extend(entries);
    }

    if all_entries.is_empty() {
        println!("No links found for the specified packs.");
        return Ok(());
    }

    output_entries(&all_entries, json);
    Ok(())
}

fn output_entries(entries: &[LinkEntry], json: bool) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(entries).unwrap_or_default()
        );
        return;
    }

    let mut by_pack: BTreeMap<&str, Vec<&LinkEntry>> = BTreeMap::new();
    for entry in entries {
        by_pack.entry(&entry.pack).or_default().push(entry);
    }

    let mut ok_total = 0usize;
    let mut missing_total = 0usize;
    let mut dangling_total = 0usize;
    let mut overwritten_total = 0usize;
    let mut drift_total = 0usize;

    for (pack_name, pack_entries) in &by_pack {
        let counts = count_statuses(pack_entries);
        ok_total += counts.0;
        missing_total += counts.1;
        dangling_total += counts.2;
        overwritten_total += counts.3;
        drift_total += counts.4;

        let has_issues = counts.1 > 0 || counts.2 > 0 || counts.3 > 0 || counts.4 > 0;
        let status_icon = if has_issues { "\u{26a0} " } else { "\u{2713} " };
        println!(
            "\n{status_icon}{pack_name}: {ok} OK, {mi} MISSING, {da} DANGLING, {ow} OVERWRITTEN, {dr} DRIFT",
            ok = counts.0,
            mi = counts.1,
            da = counts.2,
            ow = counts.3,
            dr = counts.4,
        );

        for e in pack_entries {
            if e.status != LinkStatus::Ok {
                let fixed_marker = if e.fixed { " [FIXED]" } else { "" };
                println!(
                    "  {}  {} -> {}{fixed_marker}",
                    e.status.icon(),
                    e.dst,
                    e.src,
                );
            }
        }
    }

    let total = ok_total + missing_total + dangling_total + overwritten_total + drift_total;
    println!(
        "\nTotal: {total} links ({ok_total} OK, {missing_total} MISSING, {dangling_total} DANGLING, \
         {overwritten_total} OVERWRITTEN, {drift_total} DRIFT)"
    );
}

fn count_statuses(entries: &[&LinkEntry]) -> (usize, usize, usize, usize, usize) {
    let (mut ok, mut missing, mut dangling, mut overwritten, mut drift) = (0, 0, 0, 0, 0);
    for e in entries {
        match e.status {
            LinkStatus::Ok => ok += 1,
            LinkStatus::Missing => missing += 1,
            LinkStatus::Dangling => dangling += 1,
            LinkStatus::Overwritten => overwritten += 1,
            LinkStatus::Drift => drift += 1,
        }
    }
    (ok, missing, dangling, overwritten, drift)
}

/// 检查已安装 pack 的状态一致性。
///
/// 不传 `paths` 则扫描 `state_dir` 下所有已安装 pack；
/// 传入 `paths` 则仅检查指定 pack。
pub async fn status(
    global_config: &Config,
    paths: Vec<PathBuf>,
    fix: bool,
    json: bool,
) -> Result<()> {
    if paths.is_empty() {
        status_all(fix, json).await
    } else {
        status_packs(global_config, paths, fix, json).await
    }
}
