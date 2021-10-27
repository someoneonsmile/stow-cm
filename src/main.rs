use regex::RegexSet;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::path::PathBuf;
use std::vec::Vec;
use structopt::StructOpt;
use walkdir::WalkDir;
// use std::env::current_dir;
use serde::{Deserialize, Serialize};

/// config manager (simple impl of gnu-stow)
#[derive(StructOpt, Debug)]
#[structopt(name = "stow")]
struct Opt {
    /// packages to install
    #[structopt(short = "i", long = "install")]
    to_install: Vec<PathBuf>,

    /// packages to remove
    #[structopt(short = "r", long = "remove")]
    to_remove: Vec<PathBuf>,
}

/// special config
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    /// install to target dir
    target: Option<PathBuf>,

    /// ignore file regx
    ignore: Option<Vec<String>>,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            target: Some(shell_expend_tilde("~")),
            ignore: None,
        }
    }
}

static CONFIG_FILE_NAME: &'static str = ".stowrc";

fn main() {
    let opt = Opt::from_args();
    // println!("{:?}", opt);
    // println!("{:#?}", opt);

    let common_config =
        parse_config(format!("./{}", CONFIG_FILE_NAME)).merge(&Some(Default::default()));

    remove_all(&common_config, opt.to_remove);
    install_all(&common_config, opt.to_install);
}

/// parse config file
fn parse_config<P: AsRef<Path>>(config_path: P) -> Option<Config> {
    let config_str = fs::read_to_string(config_path.as_ref()).ok()?;
    let mut config : Config = toml::from_str(&config_str).unwrap();
    if let Some(target) = config.target {
        config.target = Some(shell_expend_tilde(target));
    }
    return Some(config);
}

/// install packages
fn install_all(common_config: &Option<Config>, packs: Vec<PathBuf>) {
    for pack in packs {
        let customer_config = parse_config(&pack.join(CONFIG_FILE_NAME));
        let config = customer_config.merge(common_config).unwrap_or_default();
        install(config, pack);
    }
}

/// install packages
fn install<P: AsRef<Path>>(config: Config, pack: P) {
    println!("install pack: {:?}", pack.as_ref());
    let target = config.target.unwrap();
    let ignore_re = match &config.ignore {
        Some(ignore_regexs) => Some(RegexSet::new(ignore_regexs)),
        None => None,
    };
    let mut it = WalkDir::new(&pack).min_depth(1).into_iter();
    loop {
        let entry = match it.next() {
            None => break,
            Some(Err(err)) => panic!("ERROR: {}", err),
            Some(Ok(entry)) => entry,
        };

        if let Some(Ok(ignore_re)) = &ignore_re {
            if ignore_re.is_match(&entry.file_name().to_str().unwrap()) {
                it.skip_current_dir();
                continue;
            }
        }
        let entry_target = PathBuf::from(&target).join(
            PathBuf::from(&entry.path())
                .strip_prefix(pack.as_ref())
                .unwrap(),
        );
        if entry_target.exists() {
            if let Ok(true) = same_file::is_same_file(&entry.path(), &entry_target) {
                it.skip_current_dir();
            } else if entry_target.is_file() {
                eprintln!("target has exists, target:{:?}", entry_target);
            }
            continue;
        }
        if let Some(parent) = entry_target.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        println!("{:?} -> {:?}", entry_target, entry.path(),);
        symlink(&entry.path(), &entry_target).unwrap();
        if entry.file_type().is_dir() {
            it.skip_current_dir();
        }
    }
}

/// remove packages
fn remove_all(common_config: &Option<Config>, packs: Vec<PathBuf>) {
    for pack in packs {
        let customer_config = parse_config(&pack.join(CONFIG_FILE_NAME));
        let config = customer_config.merge(common_config).unwrap_or_default();
        remove(config, pack);
    }
}

/// remove packages
fn remove<P: AsRef<Path>>(config: Config, pack: P) {
    println!("remove pack: {:?}", pack.as_ref());
    let target = config.target.unwrap();
    let ignore_re = match &config.ignore {
        Some(ignore_regexs) => Some(RegexSet::new(ignore_regexs)),
        None => None,
    };
    let mut it = WalkDir::new(&pack).min_depth(1).into_iter();
    loop {
        let entry = match it.next() {
            None => break,
            Some(Err(err)) => panic!("ERROR: {}", err),
            Some(Ok(entry)) => entry,
        };
        if let Some(Ok(ignore_re)) = &ignore_re {
            if ignore_re.is_match(&entry.file_name().to_str().unwrap()) {
                it.skip_current_dir();
                continue;
            }
        }
        let entry_target = PathBuf::from(&target).join(
            PathBuf::from(&entry.path())
                .strip_prefix(pack.as_ref())
                .unwrap(),
        );
        if !entry_target.exists() {
            it.skip_current_dir();
            continue;
        }
        if let Ok(false) = same_file::is_same_file(&entry_target, &entry.path()) {
            eprintln!("remove symlink, not same_file, target:{:?}", entry_target);
            continue;
        }
        fs::remove_file(&entry_target).unwrap();
        if entry.file_type().is_dir() {
            it.skip_current_dir();
        }
    }
}

fn get_home_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap();
    PathBuf::from(home)
}

trait Merge<T> {
    fn merge(self, other: &T) -> T;
}

impl<T: Merge<T> + Clone> Merge<Option<T>> for Option<T> {
    fn merge(self, other: &Option<T>) -> Option<T> {
        match (self, other) {
            (Some(a), Some(b)) => Some(a.merge(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b.clone()),
            (None, None) => None,
        }
    }
}

impl Merge<Config> for Config {
    fn merge(mut self, other: &Config) -> Config {
        self.target = self.target.or_else(|| other.target.clone());
        self.ignore = match (self.ignore, &other.ignore) {
            (Some(a), Some(b)) => Some(a.merge(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b.clone()),
            (None, None) => None,
        };
        self
    }
}

impl<T: Clone> Merge<Vec<T>> for Vec<T> {
    fn merge(mut self, other: &Vec<T>) -> Vec<T> {
        self.append(&mut other.clone());
        self
    }
}

fn shell_expend_tilde<P: AsRef<Path>>(path: P) -> PathBuf {
    let mut home = get_home_dir();
    home.push(path.as_ref().strip_prefix("~/").unwrap());
    home
}
