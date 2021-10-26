use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::path::PathBuf;
use std::vec::Vec;
use structopt::StructOpt;
use walkdir::WalkDir;

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
#[derive(Debug, Clone)]
struct Config {
    /// install to target dir
    target: Option<PathBuf>,

    /// ignore file regx
    ignore: Option<Vec<String>>,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            target: Some(get_home_dir().join("temp/temp2/temp3")),
            ignore: None,
        }
    }
}

static CONFIG_FILE_NAME: &'static str = ".stowrc";

fn main() {
    let opt = Opt::from_args();
    // println!("{:?}", opt);
    // println!("{:#?}", opt);

    // let common_config = parse_config(&current_dir.join(CONFIG_FILE_NAME));
    // let content = std::fs::read_to_string(file_path.as_path()).unwrap();

    remove_all(&None, opt.to_remove);
    install_all(&None, opt.to_install);
}

/// parse config file
fn parse_config(_config_path: &Path) -> Option<Config> {
    // todo
    None
}

/// parse config file
fn merge_config(conf1: Option<Config>, conf2: &Option<Config>) -> Option<Config> {
    conf1.merge(conf2)
}

/// install packages
fn install_all(common_config: &Option<Config>, packs: Vec<PathBuf>) {
    for pack in packs {
        let customer_config = parse_config(&pack.join(CONFIG_FILE_NAME));
        let config = merge_config(customer_config, common_config).unwrap_or_default();
        install(config, pack);
    }
}

/// install packages
fn install<P: AsRef<Path>>(config: Config, pack: P) {
    println!("install pack: {:?}", pack.as_ref());
    let target = config.target.unwrap();
    let _ignore = config.ignore;
    let mut it = WalkDir::new(&pack).min_depth(1).into_iter();
    loop {
        let entry = match it.next() {
            None => break,
            Some(Err(err)) => panic!("ERROR: {}", err),
            Some(Ok(entry)) => entry,
        };
        let entry_target = PathBuf::from(&target).join(
            PathBuf::from(&entry.path())
                .strip_prefix(pack.as_ref())
                .unwrap(),
        );
        if entry_target.exists() {
            if entry_target.is_file() {
                eprintln!("target has exists, target:{:?}", entry_target);
            }
            continue;
        }
        if let Some(parent) = entry_target.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        println!(
            "create symlink, source:{:?}, target:{:?}",
            entry.path(),
            entry_target
        );
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
        let config = merge_config(customer_config, common_config).unwrap_or_default();
        remove(config, pack);
    }
}

/// remove packages
fn remove<P: AsRef<Path>>(config: Config, pack: P) {
    println!("remove pack: {:?}", pack.as_ref());
    let target = config.target.unwrap();
    let _ignore = config.ignore;
    let mut it = WalkDir::new(&pack).min_depth(1).into_iter();
    loop {
        let entry = match it.next() {
            None => break,
            Some(Err(err)) => panic!("ERROR: {}", err),
            Some(Ok(entry)) => entry,
        };
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

// fn shell_expend_tilde(path: &PathBuf) -> PathBuf {
//     let mut home = get_home_dir();
//     home.push(path.strip_prefix("~/").unwrap());
//     home
// }

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
        self.ignore = self.ignore.or_else(|| other.ignore.clone());
        self
    }
}
