use std::path::PathBuf;
use std::vec::Vec;
use structopt::StructOpt;

/// config manager (simple impl of gnu-stow)
#[derive(StructOpt, Debug)]
#[structopt(name = "stow")]
pub struct Opt {
    /// packages to install
    #[structopt(short = "i", long = "install")]
    pub to_install: Vec<PathBuf>,

    /// packages to remove
    #[structopt(short = "r", long = "remove")]
    pub to_remove: Vec<PathBuf>,
}

impl Opt {
    pub fn parse() -> Opt{
        Opt::from_args()
    }
}
