use std::path::{ Path, PathBuf };
use std::env;

pub(crate) fn get_home_dir() -> PathBuf {
    let home = env::var("HOME").unwrap();
    PathBuf::from(home)
}

pub(crate) fn shell_expend_tilde<P: AsRef<Path>>(path: P) -> PathBuf {
    let mut home = get_home_dir();
    home.push(path.as_ref().strip_prefix("~/").unwrap());
    home
}
