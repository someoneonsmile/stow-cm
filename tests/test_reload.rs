//! reload 命令集成测试。

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

mod common;

use std::sync::Arc;

use common::{TestEnv, assert_not_exists, assert_symlink, assert_track_links, write_pack_file};
use stow_cm::command::{install, reload};
use stow_cm::config::Config;

fn setup(env: &TestEnv, name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
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

fn for_pack(pack_dir: &std::path::Path) -> Arc<Config> {
    let global = Config::global().expect("global");
    Arc::new(Config::for_pack(pack_dir, &global, None, false).expect("for_pack"))
}

#[test]
fn reload_no_changes() {
    let env = TestEnv::new();
    let (pack_dir, target_dir) = setup(&env, "test-pack");
    let config = for_pack(&pack_dir);

    install(&config, &pack_dir, false).expect("install");

    assert_symlink(&target_dir.join("file_a.txt"), &pack_dir.join("file_a.txt"));
    assert_symlink(&target_dir.join("sub"), &pack_dir.join("sub"));
    assert_track_links(&pack_dir, 2);

    // reload 无变更 — 去重后链接仍存在
    let config2 = for_pack(&pack_dir);
    reload(&config2, &pack_dir, false).expect("reload");

    assert_symlink(&target_dir.join("file_a.txt"), &pack_dir.join("file_a.txt"));
    assert_symlink(&target_dir.join("sub"), &pack_dir.join("sub"));
}

#[test]
fn reload_add_file() {
    let env = TestEnv::new();
    let (pack_dir, target_dir) = setup(&env, "test-pack");
    let config = for_pack(&pack_dir);

    install(&config, &pack_dir, false).expect("install");
    assert_track_links(&pack_dir, 2);

    write_pack_file(&pack_dir, "new_file.txt", "new\n");

    let config2 = for_pack(&pack_dir);
    reload(&config2, &pack_dir, false).expect("reload");

    assert_symlink(&target_dir.join("file_a.txt"), &pack_dir.join("file_a.txt"));
    assert_symlink(&target_dir.join("sub"), &pack_dir.join("sub"));
    assert_symlink(
        &target_dir.join("new_file.txt"),
        &pack_dir.join("new_file.txt"),
    );
    // reload 去重：sub 的 remove+create 抵消，track 仅记录新增文件
    assert_track_links(&pack_dir, 1);
}

#[test]
fn reload_remove_file() {
    let env = TestEnv::new();
    let (pack_dir, target_dir) = setup(&env, "test-pack");
    let config = for_pack(&pack_dir);

    install(&config, &pack_dir, false).expect("install");
    assert_track_links(&pack_dir, 2);

    std::fs::remove_file(pack_dir.join("file_a.txt")).expect("remove file_a.txt");

    let config2 = for_pack(&pack_dir);
    reload(&config2, &pack_dir, false).expect("reload");

    assert_not_exists(&target_dir.join("file_a.txt"));
    assert_symlink(&target_dir.join("sub"), &pack_dir.join("sub"));
    // reload 去重：file_a.txt 移除，sub remove+create 抵消 → track 0 条
    assert_track_links(&pack_dir, 0);
}

#[test]
fn reload_fresh() {
    let env = TestEnv::new();
    let (pack_dir, target_dir) = setup(&env, "test-pack");
    let config = for_pack(&pack_dir);

    reload(&config, &pack_dir, false).expect("reload");

    assert_symlink(&target_dir.join("file_a.txt"), &pack_dir.join("file_a.txt"));
    assert_symlink(&target_dir.join("sub"), &pack_dir.join("sub"));
    assert_track_links(&pack_dir, 2);
}

#[test]
fn reload_dry_run() {
    let env = TestEnv::new();
    let (pack_dir, target_dir) = setup(&env, "test-pack");
    let config = for_pack(&pack_dir);

    install(&config, &pack_dir, false).expect("install");

    std::fs::remove_file(pack_dir.join("file_a.txt")).expect("remove file_a.txt");
    write_pack_file(&pack_dir, "added.txt", "added\n");

    let config2 = for_pack(&pack_dir);
    reload(&config2, &pack_dir, true).expect("reload dry_run");

    assert_symlink(&target_dir.join("file_a.txt"), &pack_dir.join("file_a.txt"));
    assert_symlink(&target_dir.join("sub"), &pack_dir.join("sub"));
    assert_not_exists(&target_dir.join("added.txt"));
    assert_track_links(&pack_dir, 2);
}
