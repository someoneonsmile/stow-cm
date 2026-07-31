#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

mod common;

use std::sync::Arc;

use stow_cm::command::{install, remove};
use stow_cm::config::Config;

fn setup_separated(env: &common::TestEnv, name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let target_dir = env.config_path().join(name);
    std::fs::create_dir_all(&target_dir).unwrap();

    let pack_base = env.state_path().join("packs");
    std::fs::create_dir_all(&pack_base).unwrap();
    let pack_dir = pack_base.join(name);
    std::fs::create_dir_all(&pack_dir).unwrap();

    let config_toml = format!(
        "target = '{}'\nmode = 'symlink'\n",
        target_dir.to_string_lossy()
    );
    std::fs::write(pack_dir.join("stow-cm.toml"), config_toml).unwrap();

    common::write_pack_file(&pack_dir, "file_a.txt", "content a\n");
    common::write_pack_file(&pack_dir, "sub/file_b.txt", "content b\n");

    (std::fs::canonicalize(&pack_dir).unwrap(), target_dir)
}

#[test]
fn test_remove_basic() {
    let env = common::TestEnv::new();
    let (pack_dir, target_dir) = setup_separated(&env, "test-remove-basic");
    let global = Config::global().expect("global");
    let config = Arc::new(Config::for_pack(&pack_dir, &global, None, false).expect("for_pack"));

    install(&config, &pack_dir, false).expect("install");

    common::assert_exists(&target_dir.join("file_a.txt"));
    common::assert_exists(&target_dir.join("sub").join("file_b.txt"));

    let track_path = common::assert_track_links(&pack_dir, 2);

    remove(&config, &pack_dir, false).expect("remove");

    common::assert_not_exists(&target_dir.join("file_a.txt"));
    common::assert_not_exists(&target_dir.join("sub").join("file_b.txt"));
    common::assert_not_exists(&track_path);
}

#[test]
fn test_remove_dry_run() {
    let env = common::TestEnv::new();
    let (pack_dir, target_dir) = setup_separated(&env, "test-remove-dryrun");
    let global = Config::global().expect("global");
    let config = Arc::new(Config::for_pack(&pack_dir, &global, None, false).expect("for_pack"));

    install(&config, &pack_dir, false).expect("install");

    remove(&config, &pack_dir, true).expect("dry-run remove");

    common::assert_exists(&target_dir.join("file_a.txt"));
    common::assert_exists(&target_dir.join("sub").join("file_b.txt"));
    common::assert_track_links(&pack_dir, 2);
}

#[test]
fn test_remove_non_installed() {
    let env = common::TestEnv::new();
    let (pack_dir, _) = setup_separated(&env, "test-remove-notinst");
    let global = Config::global().expect("global");
    let config = Arc::new(Config::for_pack(&pack_dir, &global, None, false).expect("for_pack"));

    let result = remove(&config, &pack_dir, false);
    assert!(
        result.is_ok(),
        "removing non-installed pack should not error"
    );
}
