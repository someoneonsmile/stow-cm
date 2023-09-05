use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::vec::Vec;

/// config manager (gnu-stow like)
#[derive(Parser, Debug)]
#[command(version, about, name = "stow-cm")]
#[command(arg_required_else_help = true)]
// #[command(propagate_version = true)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
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
    /// Scan and clean all symbol that link to pack from pack target
    #[command(arg_required_else_help = true)]
    Clean {
        #[arg(name = "PACK_PATH")]
        paths: Vec<PathBuf>,
    },
}
