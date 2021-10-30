use regex::RegexSet;
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::path::PathBuf;
use std::vec::Vec;
use structopt::StructOpt;

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

/// pack config
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
            ignore: Some(vec![CONFIG_FILE_NAME.to_string()]),
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
    let mut config: Config = toml::from_str(&config_str).unwrap();
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
        Some(ignore_regexs) => Some(RegexSet::new(ignore_regexs).unwrap()),
        None => None,
    };

    let mut paths = Vec::new();
    for path in fs::read_dir(pack.as_ref()).unwrap() {
        let (_, sub_path_option) = CollectBot::new(path.unwrap().path(), &ignore_re).collect();
        if let Some(mut sub_paths) = sub_path_option {
            paths.append(&mut sub_paths);
        }
    }

    for path in paths {
        let entry_target = PathBuf::from(&target).join(path.strip_prefix(pack.as_ref()).unwrap());
        if entry_target.exists() {
            if let Ok(false) = same_file::is_same_file(&path, &entry_target) {
                eprintln!("target has exists, target:{:?}", entry_target);
            }
            continue;
        }
        if let Some(parent) = entry_target.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        println!("{:?} -> {:?}", entry_target, path);
        symlink(&path, &entry_target).unwrap();
    }
}

struct CollectBot<'a> {
    path: PathBuf,
    ignore: &'a Option<RegexSet>,
}

impl<'a> CollectBot<'a> {
    fn new<P: AsRef<Path>>(path: P, ignore: &'a Option<RegexSet>) -> Self {
        CollectBot {
            path: path.as_ref().to_path_buf(),
            ignore,
        }
    }

    /// 从树的叶子节点回溯
    /// 没有 Ignore 的时候, 折叠目录
    /// 返回当前根节点
    fn collect(self) -> (bool, Option<Vec<PathBuf>>) {
        if !self.path.exists() {
            return (false, None);
        }

        if let Some(ignore_re) = &self.ignore {
            if ignore_re.is_match(self.path.file_name().unwrap().to_str().unwrap()) {
                return (true, None);
            }
        }

        if self.path.is_file() {
            return (false, Some(vec![self.path]));
        }

        let mut has_ignore = false;

        let mut paths = Vec::new();
        for path in fs::read_dir(&self.path).unwrap() {
            let (sub_ignore, sub_paths_option) =
                CollectBot::new(&path.unwrap().path(), &self.ignore).collect();
            has_ignore |= sub_ignore;
            if let Some(mut sub_paths) = sub_paths_option {
                paths.append(&mut sub_paths)
            }
        }

        if !has_ignore {
            return (false, Some(vec![self.path]));
        }

        return (true, Some(paths));
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
        Some(ignore_regexs) => Some(RegexSet::new(ignore_regexs).unwrap()),
        None => None,
    };

    let mut paths = Vec::new();
    for path in fs::read_dir(&pack).unwrap() {
        let (_, sub_path_option) = CollectBot::new(&path.unwrap().path(), &ignore_re).collect();
        if let Some(mut sub_paths) = sub_path_option {
            paths.append(&mut sub_paths);
        }
    }
    for path in paths {
        let entry_target = PathBuf::from(&target).join(
            PathBuf::from(&path)
                .strip_prefix(pack.as_ref())
                .unwrap(),
        );
        if !entry_target.exists() {
            continue;
        }
        if let Ok(false) = same_file::is_same_file(&entry_target, &path) {
            eprintln!("remove symlink, not same_file, target:{:?}", entry_target);
            continue;
        }
        fs::remove_file(&entry_target).unwrap();
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
