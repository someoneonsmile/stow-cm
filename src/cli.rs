use std::path::PathBuf;
use std::vec::Vec;
use structopt::StructOpt;

/// config manager (gnu-stow like)
#[derive(StructOpt, Debug)]
#[structopt(name = "stow-cm")]
pub(crate) struct Opt {
    /// packages to install
    #[structopt(short = "i", long = "install")]
    pub to_install: Option<Vec<PathBuf>>,

    /// packages to remove
    #[structopt(short = "d", long = "remove")]
    pub to_remove: Option<Vec<PathBuf>>,

    /// packages to unlink
    #[structopt(short = "u", long = "unlink")]
    pub to_unlink: Option<Vec<PathBuf>>,

    /// packages to reload
    #[structopt(short = "r", long = "reload")]
    pub to_reload: Option<Vec<PathBuf>>,
    // force replace
    // #[structopt(short = "f", long = "force", parse(from_flag))]
    // pub force: bool,
}

impl Opt {
    pub fn parse() -> Opt {
        Opt::from_args()
    }
}
