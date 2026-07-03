// ⚠️ 此文件被 build.rs 通过 include! 引入，禁止添加 use crate:: 依赖。
// 如需 crate 级别引用，请在 cli.rs 中添加。

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// config manager (gnu-stow like)
#[derive(Parser, Debug)]
#[command(version, about, name = "stow-cm")]
#[command(arg_required_else_help = true)]
pub struct Cli {
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
    },
    /// Reload packs (remove and install)
    #[command(arg_required_else_help = true)]
    Reload {
        #[arg(name = "PACK_PATH")]
        paths: Vec<PathBuf>,
    },
    /// Scan and clean all symlinks that link from pack to pack target
    #[command(arg_required_else_help = true)]
    Clean {
        #[arg(name = "PACK_PATH")]
        paths: Vec<PathBuf>,
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
}
