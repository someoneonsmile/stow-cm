use std::env;

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
    pack_id: String,
    pack: String,
    #[serde(rename = "from")]
    pack_path: String,
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

        let pack_path = track
            .pack_path
            .as_ref()
            .map_or_else(|| "-".to_string(), |p| p.display().to_string());

        entries.push(PackEntry {
            pack_id: pack_id.to_string(),
            pack: pack_name,
            pack_path,
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

/// 计算能唯一区分所有 `PACK_ID` 的最短前缀长度（类似 git 的 abbrev-commit）
/// 下限为 7，上限为完整长度
fn abbrev_len(entries: &[PackEntry]) -> usize {
    const MIN_LEN: usize = 7;
    let max_possible = entries
        .iter()
        .map(|e| e.pack_id.len())
        .max()
        .unwrap_or(MIN_LEN);

    let mut len = MIN_LEN;
    while len < max_possible {
        let mut seen = std::collections::HashSet::<String>::new();
        let all_unique = entries.iter().all(|e| seen.insert(prefix(&e.pack_id, len)));
        if all_unique {
            break;
        }
        len += 1;
    }
    len
}

#[inline]
fn prefix(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn terminal_width() -> usize {
    env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(120)
}

fn replace_home(path: &str, home: &str) -> String {
    if home.is_empty() {
        return path.to_string();
    }
    if path == home {
        return "~".to_string();
    }
    path.replacen(&format!("{home}/"), "~/", 1)
}

/// 目录段缩写长度：隐藏目录（`.config`）保留 `.c`，普通目录保留首字符
#[inline]
fn abbrev_seg_len(seg: &str) -> usize {
    if seg.starts_with('.') { 2 } else { 1 }
}

/// 折叠过长路径，每级目录至少保留一个字符
///
/// 策略递进：
/// 1. `首段/m/i/d/尾段` — 每级保留首字符
/// 2. `head…tail` — 等分字符截断兜底
fn fold_path(path: &str, max_width: usize) -> String {
    if path.len() <= max_width {
        return path.to_string();
    }
    let segments: Vec<&str> = path.split('/').collect();
    if segments.len() < 3 {
        return char_truncate(path, max_width);
    }

    // 策略1: 每级保留首字符（首段完整 + 中间首字符 + 尾段完整）
    let head = segments.first().copied().unwrap_or("");
    let tail = segments.last().copied().unwrap_or("");
    let middle: String = segments
        .get(1..segments.len().saturating_sub(1))
        .map(|s| {
            s.iter()
                .map(|seg| seg.chars().take(abbrev_seg_len(seg)).collect::<String>())
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();
    let candidate = if middle.is_empty() {
        format!("{head}/{tail}")
    } else {
        format!("{head}/{middle}/{tail}")
    };
    if candidate.len() <= max_width {
        return candidate;
    }

    // 策略2: 退化为等分字符截断
    char_truncate(path, max_width)
}

#[inline]
fn char_truncate(path: &str, max_width: usize) -> String {
    let ellipsis = "…";
    let keep = (max_width.saturating_sub(ellipsis.len())) / 2;
    let chars: Vec<char> = path.chars().collect();
    let head: String = chars.iter().take(keep).collect();
    let tail: String = chars
        .iter()
        .rev()
        .take(keep)
        .copied()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{head}…{tail}")
}

fn print_table(entries: &[PackEntry]) {
    let home = dirs::home_dir()
        .and_then(|h| h.to_str().map(String::from))
        .unwrap_or_default();
    let term_w = terminal_width();

    let id_len = abbrev_len(entries);
    let max_pack = entries
        .iter()
        .map(|e| e.pack.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let max_links_digits = entries
        .iter()
        .map(|e| e.links.to_string().len())
        .max()
        .unwrap_or(5)
        .max(5);

    // 固定列宽：id + pack + links + mode(8) + encrypted(3) + 分隔符(6*2=12)
    let fixed_w = id_len + max_pack + max_links_digits + 8 + 3 + 12;
    let remain = term_w.saturating_sub(fixed_w);
    let max_from = (remain / 2).max(4);
    let max_target = (remain.saturating_sub(max_from)).max(6);

    let display_paths: Vec<(String, String)> = entries
        .iter()
        .map(|e| {
            let from = replace_home(&e.pack_path, &home);
            let target = replace_home(&e.target, &home);
            (fold_path(&from, max_from), fold_path(&target, max_target))
        })
        .collect();

    let actual_from = display_paths
        .iter()
        .map(|(f, _)| f.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let actual_target = display_paths
        .iter()
        .map(|(_, t)| t.len())
        .max()
        .unwrap_or(6)
        .max(6);

    println!(
        "{:<id_w$}  {:<pack_w$}  {:<from_w$}  {:<target_w$}  {:>links_w$}  {:<8}  ENCRYPTED",
        "PACK_ID",
        "PACK",
        "FROM",
        "TARGET",
        "LINKS",
        "MODE",
        id_w = id_len,
        pack_w = max_pack,
        from_w = actual_from,
        target_w = actual_target,
        links_w = max_links_digits,
    );

    for (entry, (from, target)) in entries.iter().zip(&display_paths) {
        println!(
            "{:<id_w$}  {:<pack_w$}  {:<from_w$}  {:<target_w$}  {:>links_w$}  {:<8}  {}",
            prefix(&entry.pack_id, id_len),
            entry.pack,
            from,
            target,
            entry.links,
            entry.mode,
            if entry.encrypted { "yes" } else { "no" },
            id_w = id_len,
            pack_w = max_pack,
            from_w = actual_from,
            target_w = actual_target,
            links_w = max_links_digits,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abbrev_seg_len_normal() {
        assert_eq!(abbrev_seg_len("fish"), 1);
        assert_eq!(abbrev_seg_len("dotfiles"), 1);
        assert_eq!(abbrev_seg_len("n"), 1);
    }

    #[test]
    fn abbrev_seg_len_hidden() {
        assert_eq!(abbrev_seg_len(".config"), 2);
        assert_eq!(abbrev_seg_len(".local"), 2);
        assert_eq!(abbrev_seg_len(".c"), 2);
    }

    #[test]
    fn abbrev_seg_len_dot_only() {
        assert_eq!(abbrev_seg_len("."), 2);
        assert_eq!(abbrev_seg_len(".."), 2);
    }

    #[test]
    fn replace_home_basic() {
        assert_eq!(
            replace_home("/home/user/.config", "/home/user"),
            "~/.config"
        );
        assert_eq!(replace_home("/home/user", "/home/user"), "~");
        assert_eq!(
            replace_home("/home/user2/data", "/home/user"),
            "/home/user2/data"
        );
        assert_eq!(replace_home("/other/path", ""), "/other/path");
    }

    #[test]
    fn fold_path_no_truncation() {
        assert_eq!(fold_path("~/a/b/c", 20), "~/a/b/c");
    }

    #[test]
    fn fold_path_abbrev_normal() {
        assert_eq!(fold_path("~/dotfiles/stow/nvim", 16), "~/d/s/nvim");
    }

    #[test]
    fn fold_path_abbrev_hidden_dirs() {
        // .config → .c, .local → .l
        assert_eq!(fold_path("~/.config/fish", 13), "~/.c/fish");
        assert_eq!(fold_path("~/.local/share/stow-cm", 20), "~/.l/s/stow-cm");
    }

    #[test]
    fn fold_path_head_middle_separator() {
        // head(~) 和 middle 之间必须有 /
        let result = fold_path("~/alpha/beta/gamma", 14);
        assert!(result.starts_with("~/"), "expected '~/', got '{result}'");
    }

    #[test]
    fn fold_path_char_truncate_fallback() {
        // 首字符形式仍超宽时退化为等分截断
        let result = fold_path("~/a/b/c", 6);
        assert!(result.contains('…'));
        assert!(result.len() <= 6);
    }
}
