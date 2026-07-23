#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

mod common;

use std::sync::Arc;

use common::{TestEnv, write_pack_file};
use stow_cm::command::{install, list};
use stow_cm::config::Config;

fn setup(env: &TestEnv, name: &str) {
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
    write_pack_file(&pack_dir, "file.txt", "content\n");

    let pack_dir = std::fs::canonicalize(&pack_dir).unwrap();
    let global = Config::global().expect("global");
    let config = Arc::new(Config::for_pack(&pack_dir, &global, None, false).expect("for_pack"));
    install(&config, &pack_dir, false).expect("install");
}

#[test]
fn test_list_empty() {
    let _env = TestEnv::new();
    list(false).expect("list should succeed on empty state");
}

#[test]
fn test_list_with_packs() {
    let env = TestEnv::new();
    setup(&env, "test-list-pack1");
    setup(&env, "test-list-pack2");
    list(false).expect("list should succeed");
}

#[test]
fn test_list_json() {
    let env = TestEnv::new();
    setup(&env, "test-list-json");
    list(true).expect("list --json should succeed");
}
