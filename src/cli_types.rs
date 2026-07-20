// ⚠️ 此文件被 build.rs 通过 include! 引入，禁止添加 use crate:: 依赖。
// 如需 crate 级别引用，请在 cli.rs 中添加。

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// config manager (gnu-stow like)
#[derive(Parser, Debug)]
#[command(version, about, name = "stow-cm")]
#[command(arg_required_else_help = true)]
pub struct Cli {
    /// Increase log verbosity (-v debug, -vv trace)
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Quiet mode, only output errors
    #[arg(short = 'q', long = "quiet", action = clap::ArgAction::SetTrue, global = true, conflicts_with = "verbose")]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Install packs
    #[command(arg_required_else_help = true)]
    Install {
        #[arg(name = "PACK_PATH")]
        paths: Vec<PathBuf>,
    },
    /// Remove packs
    #[command(arg_required_else_help = true)]
    Remove {
        #[arg(name = "PACK_PATH")]
        paths: Vec<PathBuf>,
        /// Remove by `PACK_ID` instead of filesystem path
        #[arg(long = "id", value_name = "PACK_ID")]
        ids: Vec<String>,
    },
    /// Reload packs (remove and install)
    #[command(arg_required_else_help = true)]
    Reload {
        #[arg(name = "PACK_PATH")]
        paths: Vec<PathBuf>,
        /// Reload by `PACK_ID` instead of filesystem path
        #[arg(long = "id", value_name = "PACK_ID")]
        ids: Vec<String>,
    },
    /// Scan and clean all symlinks that link from pack to pack target
    #[command(arg_required_else_help = true)]
    Clean {
        #[arg(name = "PACK_PATH")]
        paths: Vec<PathBuf>,
        /// Clean by `PACK_ID` instead of filesystem path
        #[arg(long = "id", value_name = "PACK_ID")]
        ids: Vec<String>,
    },
    /// Scan files in the given pack for replacement variables, encrypt them,
    /// and replace them back to the original files
    #[command(arg_required_else_help = true)]
    Encrypt {
        #[arg(name = "PACK_PATH")]
        paths: Vec<PathBuf>,
    },
    /// Scan files in the given pack for replacement variables, decrypt them,
    /// and replace them back to the original files
    #[command(arg_required_else_help = true)]
    Decrypt {
        #[arg(name = "PACK_PATH")]
        paths: Vec<PathBuf>,
    },
    /// List all installed packs and their status
    List {
        /// Output in JSON format
        #[arg(long = "json")]
        json: bool,
    },
    /// Adopt existing config directories into stow management (reverse takeover)
    #[command(arg_required_else_help = true)]
    Adopt {
        /// Source directories to adopt (e.g. ~/.config/fish ~/.config/nvim)
        #[arg(name = "SOURCE_DIR", required = true)]
        sources: Vec<PathBuf>,
        /// Stow repository directory (packs will be created here)
        #[arg(short = 't', long = "to", value_name = "STOW_DIR")]
        to: PathBuf,
    },
    /// Check consistency between installed links and the filesystem
    Status {
        /// Optional pack paths; if omitted, check all installed packs
        #[arg(name = "PACK_PATH")]
        paths: Vec<PathBuf>,
        /// Auto-fix repairable issues (recreate missing links)
        #[arg(long = "fix")]
        fix: bool,
        /// Output in JSON format
        #[arg(long = "json")]
        json: bool,
    },
}
