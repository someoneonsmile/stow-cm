use std::path::PathBuf;
use std::vec::Vec;
use structopt::StructOpt;

/// config manager (simple impl of gnu-stow)
#[derive(StructOpt, Debug)]
#[structopt(name = "stow")]
pub struct Opt {
    /// packages to install
    #[structopt(short = "i", long = "install")]
    pub to_install: Option<Vec<PathBuf>>,

    /// packages to remove
    #[structopt(short = "d", long = "remove")]
    pub to_remove: Option<Vec<PathBuf>>,

    /// packages to install
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
