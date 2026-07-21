use std::borrow::Cow;
use std::fmt::Write;
use std::path::Path;

use anyhow::{anyhow, bail};
use inquire::{Confirm, Select, Text};
use log::info;

use crate::config::Config;
use crate::constants::CONFIG_FILE_NAME;
use crate::error::Result;
use crate::paths::default_pack_target;
use crate::util;

const DEFAULT_TEMPLATE: &str = include_str!("../../templates/default-config.toml");
const REFERENCE_TEMPLATE: &str = include_str!("../../templates/reference-config.toml");

struct PackMeta<'a> {
    pack_name: &'a str,
    default_name: &'a str,
    target: &'a str,
    target_is_default: bool,
    raw_target: Option<&'a str>,
    resolved_target: Option<&'a str>,
    mode: &'a str,
    fold: bool,
    encryption: bool,
}

struct Gathered {
    pack_name: String,
    target: String,
    target_is_default: bool,
    raw_target: Option<String>,
    resolved_target: Option<String>,
    mode: String,
    fold: bool,
    encryption: bool,
}

pub fn init(pack_path: &Path, global: Option<&Config>, use_defaults: bool) -> Result<()> {
    let default_name = util::pack_name(pack_path)?;
    let config_path = pack_path.join(CONFIG_FILE_NAME);

    if config_path.exists() {
        bail!(
            "{default_name}: {CONFIG_FILE_NAME} already exists at '{}'",
            config_path.display()
        );
    }

    if use_defaults {
        let global = global.ok_or_else(|| anyhow!("global config not loaded"))?;
        std::fs::create_dir_all(pack_path).map_err(|e| {
            anyhow!(
                "{default_name}: failed to create directory '{}': {e}",
                pack_path.display()
            )
        })?;
        write_default_config(&config_path, global, pack_path, &default_name)?;
        info!("{default_name}: created {}", config_path.display());
    } else {
        let global = global.ok_or_else(|| anyhow!("global config not loaded"))?;
        let gathered = gather_interactive(global, pack_path, &default_name)?;

        std::fs::create_dir_all(pack_path).map_err(|e| {
            anyhow!(
                "{}: failed to create directory '{}': {e}",
                gathered.pack_name,
                pack_path.display()
            )
        })?;

        let meta = PackMeta {
            pack_name: &gathered.pack_name,
            default_name: &default_name,
            target: &gathered.target,
            target_is_default: gathered.target_is_default,
            raw_target: gathered.raw_target.as_deref(),
            resolved_target: gathered.resolved_target.as_deref(),
            mode: &gathered.mode,
            fold: gathered.fold,
            encryption: gathered.encryption,
        };
        write_config(&config_path, &meta)?;
        info!("{}: created {}", gathered.pack_name, config_path.display());
    }

    Ok(())
}

fn write_default_config(
    config_path: &Path,
    global: &Config,
    pack_path: &Path,
    pack_name: &str,
) -> Result<()> {
    let resolved = Config::for_pack(pack_path, global, None, true)?;
    let resolved_target = resolved
        .target
        .as_ref()
        .map_or_else(default_pack_target, |p| p.to_string_lossy().to_string());

    let raw_target = global
        .target
        .as_ref()
        .map_or_else(default_pack_target, |p| p.to_string_lossy().to_string());

    let content = DEFAULT_TEMPLATE
        .replace("__PACK_NAME__", pack_name)
        .replace("__TARGET_RAW__", &raw_target)
        .replace("__TARGET__", &resolved_target);

    std::fs::write(config_path, &content)
        .map_err(|e| anyhow!("{pack_name}: failed to write {CONFIG_FILE_NAME}: {e}"))?;
    Ok(())
}

fn gather_interactive(global: &Config, pack_path: &Path, default_name: &str) -> Result<Gathered> {
    let pack_name = Text::new("Pack name:")
        .with_default(default_name)
        .prompt()
        .map_err(|e| anyhow!("{e}"))?;

    let resolved = Config::for_pack(pack_path, global, Some(&pack_name), true)?;
    let resolved_target = resolved
        .target
        .as_ref()
        .map(|p| p.to_string_lossy().to_string());
    let raw_target = global
        .target
        .as_ref()
        .map(|p| p.to_string_lossy().to_string());
    let (target_str, target_is_default) = get_target_choice(resolved_target.as_ref())?;

    let mode = Select::new("Mode:", vec!["symlink", "copy"])
        .with_help_message(
            "copy mode is not compatible with the `clean` command — use `remove` to uninstall",
        )
        .prompt()
        .map_err(|e| anyhow!("{e}"))?;

    let fold = if mode == "symlink" {
        Confirm::new("Enable directory folding?")
            .with_default(true)
            .with_help_message(
                "fold = true: symlink the whole directory; fold = false: expand per file",
            )
            .prompt()
            .map_err(|e| anyhow!("{e}"))?
    } else {
        false
    };

    let enable_encryption = Confirm::new("Enable encryption?")
        .with_default(false)
        .prompt()
        .map_err(|e| anyhow!("{e}"))?;

    Ok(Gathered {
        pack_name,
        target: target_str,
        target_is_default,
        raw_target,
        resolved_target,
        mode: mode.to_owned(),
        fold,
        encryption: enable_encryption,
    })
}

fn get_target_choice(resolved_target: Option<&String>) -> Result<(String, bool)> {
    if let Some(target) = resolved_target {
        let options: Vec<Cow<'_, str>> = vec![
            Cow::Owned(format!("Use global default → {target}")),
            Cow::Borrowed("Custom path"),
        ];
        let choice = Select::new("Target directory:", options)
            .prompt()
            .map_err(|e| anyhow!("{e}"))?;
        if choice.as_ref().starts_with("Use global") {
            Ok((String::new(), true))
        } else {
            let custom = Text::new("Target path (where pack files will be linked/copied to):")
                .prompt()
                .map_err(|e| anyhow!("{e}"))?;
            Ok((custom, false))
        }
    } else {
        let custom = Text::new("Target path (where pack files will be linked/copied to):")
            .prompt()
            .map_err(|e| anyhow!("{e}"))?;
        Ok((custom, false))
    }
}

fn escape_toml_string(s: &str) -> String {
    if s.contains('\'') {
        format!("\"{s}\"")
    } else {
        format!("'{s}'")
    }
}

fn write_config(config_path: &Path, meta: &PackMeta<'_>) -> Result<()> {
    let mut content = String::new();

    let _ = writeln!(content, "# stow-cm config for {}", meta.pack_name);

    if meta.pack_name != meta.default_name {
        let _ = writeln!(content, "name = \"{}\"", meta.pack_name);
    }

    if meta.target_is_default {
        let raw = meta.raw_target.unwrap_or(meta.target);
        let resolved = meta.resolved_target.unwrap_or(meta.target);
        let _ = writeln!(
            content,
            "# target inherits from global config (default: {raw})\n# target = {}",
            escape_toml_string(resolved),
        );
    } else {
        let _ = writeln!(content, "target = {}", escape_toml_string(meta.target));
    }

    let _ = writeln!(content, "mode = '{}'", meta.mode);
    let _ = writeln!(
        content,
        "# symlink: link per file/dir; copy: duplicate files to target"
    );

    if meta.mode == "symlink" && !meta.fold {
        let _ = writeln!(
            content,
            "fold = false                # disable directory folding: expand per file"
        );
    }

    if meta.encryption {
        content.push_str("\n[encrypted]\nenable = true\n");
    }

    content.push('\n');
    let template = if meta.mode == "symlink" && !meta.fold {
        REFERENCE_TEMPLATE
            .lines()
            .filter(|line| !line.contains("fold = true"))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        REFERENCE_TEMPLATE.to_owned()
    };
    content.push_str(&template);

    std::fs::write(config_path, &content).map_err(|e| {
        anyhow!(
            "{}: failed to write {CONFIG_FILE_NAME}: {e}",
            meta.pack_name
        )
    })?;

    Ok(())
}
