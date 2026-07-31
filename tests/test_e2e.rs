//! 端到端集成测试：验证多个命令协同工作的完整生命周期。

#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

mod common;

use std::path::PathBuf;
use std::sync::Arc;

use common::{TestEnv, assert_not_exists, assert_symlink, assert_track_links};
use stow_cm::command::{clean, install, reload, remove, status};
use stow_cm::config::Config;

fn for_pack(pack_dir: &PathBuf, global: &Arc<Config>) -> Arc<Config> {
    Arc::new(Config::for_pack(pack_dir, global, None, false).expect("for_pack"))
}

/// pack 放在 state 下，target 放在 config 下，两者分离避免自冲突。
fn setup_pack(
    env: &TestEnv,
    name: &str,
    config_toml: &str,
    files: &[(&str, &str)],
) -> (PathBuf, PathBuf, Arc<Config>) {
    let target_dir = env.config_path().join(name);
    std::fs::create_dir_all(&target_dir).unwrap();

    let pack_base = env.state_path().join("packs");
    std::fs::create_dir_all(&pack_base).unwrap();
    let pack_dir = pack_base.join(name);
    std::fs::create_dir_all(&pack_dir).unwrap();
    std::fs::write(pack_dir.join("stow-cm.toml"), config_toml).unwrap();

    for (rel_path, content) in files {
        common::write_pack_file(&pack_dir, rel_path, content);
    }

    let global = Arc::new(Config::global().expect("global config"));
    let pack_dir = std::fs::canonicalize(&pack_dir).expect("canonicalize");
    let cfg = for_pack(&pack_dir, &global);

    (pack_dir, target_dir, cfg)
}

fn symlink_config(target: &str) -> String {
    format!("target = '{target}'\nmode = 'symlink'\n")
}

// ── 测试 1: install → status → reload → clean ──

#[test]
fn full_lifecycle_symlink() {
    let env = TestEnv::new();
    let (pack_dir, target_dir, cfg) = setup_pack(
        &env,
        "e2e-symlink",
        &symlink_config(&env.config_path().join("e2e-symlink").to_string_lossy()),
        &[
            ("one.txt", "one\n"),
            ("two.txt", "two\n"),
            ("sub/three.txt", "three\n"),
        ],
    );

    install(&cfg, &pack_dir, false).expect("install");

    // 有 sub 目录时 fold=true，sub 被折叠为目录级 symlink
    let link_one = target_dir.join("one.txt");
    let link_two = target_dir.join("two.txt");
    let link_sub = target_dir.join("sub");
    assert_symlink(&link_one, pack_dir.join("one.txt"));
    assert_symlink(&link_two, pack_dir.join("two.txt"));
    assert_symlink(&link_sub, pack_dir.join("sub"));

    let track_path = assert_track_links(&pack_dir, 3);

    let global = Config::global().expect("global");
    status(&global, vec![pack_dir.clone()], false, false).expect("status");

    let cfg2 = for_pack(&pack_dir, &Arc::new(Config::global().expect("global")));
    reload(&cfg2, &pack_dir, false).expect("reload");

    assert_symlink(&link_one, pack_dir.join("one.txt"));
    assert_symlink(&link_two, pack_dir.join("two.txt"));
    assert_symlink(&link_sub, pack_dir.join("sub"));

    clean(&cfg, &pack_dir, false).expect("clean");

    assert_not_exists(&link_one);
    assert_not_exists(&link_two);
    assert_not_exists(&link_sub);
    assert_not_exists(&track_path);
}

// ── 测试 2: install → status → remove ──

#[test]
fn full_lifecycle_remove() {
    let env = TestEnv::new();
    let (pack_dir, target_dir, cfg) = setup_pack(
        &env,
        "e2e-remove",
        &symlink_config(&env.config_path().join("e2e-remove").to_string_lossy()),
        &[("a.txt", "a\n"), ("b.txt", "b\n")],
    );

    install(&cfg, &pack_dir, false).expect("install");

    let link_a = target_dir.join("a.txt");
    let link_b = target_dir.join("b.txt");
    assert_symlink(&link_a, pack_dir.join("a.txt"));
    assert_symlink(&link_b, pack_dir.join("b.txt"));

    let track_path = assert_track_links(&pack_dir, 2);

    let global = Config::global().expect("global");
    status(&global, vec![pack_dir.clone()], false, false).expect("status");

    remove(&cfg, &pack_dir, false).expect("remove");

    assert_not_exists(&link_a);
    assert_not_exists(&link_b);
    assert_not_exists(&track_path);
}

// ── 测试 3: ignore + override ──

#[test]
fn ignore_and_override_together() {
    let env = TestEnv::new();
    let target_name = "e2e-ignore-override";
    let target_dir = env.config_path().join(target_name);
    std::fs::create_dir_all(&target_dir).unwrap();

    let config = format!(
        r#"target = '{t}'
mode = 'symlink'
ignore = ['.*\.md', 'ignore-me\.tmp']
override = ['.*\.conf']
"#,
        t = target_dir.to_string_lossy(),
    );

    let pack_base = env.state_path().join("packs");
    std::fs::create_dir_all(&pack_base).unwrap();
    let pack_dir = pack_base.join(target_name);
    std::fs::create_dir_all(&pack_dir).unwrap();
    std::fs::write(pack_dir.join("stow-cm.toml"), &config).unwrap();

    common::write_pack_file(&pack_dir, "normal.txt", "normal\n");
    common::write_pack_file(&pack_dir, "readme.md", "# readme\n");
    common::write_pack_file(&pack_dir, "ignore-me.tmp", "tmp\n");
    common::write_pack_file(&pack_dir, "settings.conf", "override settings\n");
    common::write_pack_file(&pack_dir, "sub/other.conf", "other config\n");

    // 预创建 override 匹配的文件
    std::fs::write(target_dir.join("settings.conf"), "pre-existing\n").unwrap();
    std::fs::create_dir_all(target_dir.join("sub")).unwrap();
    std::fs::write(target_dir.join("sub/other.conf"), "pre-existing sub\n").unwrap();

    let global = Arc::new(Config::global().expect("global"));
    let pack_dir = std::fs::canonicalize(&pack_dir).expect("canonicalize");
    let cfg = for_pack(&pack_dir, &global);

    install(&cfg, &pack_dir, false).expect("install");

    assert_not_exists(target_dir.join("readme.md"));
    assert_not_exists(target_dir.join("ignore-me.tmp"));

    assert_symlink(&target_dir.join("normal.txt"), pack_dir.join("normal.txt"));
    assert_symlink(
        &target_dir.join("settings.conf"),
        pack_dir.join("settings.conf"),
    );
    assert_symlink(&target_dir.join("sub"), pack_dir.join("sub"));

    let content = std::fs::read_to_string(target_dir.join("settings.conf")).unwrap();
    assert_eq!(content, "override settings\n");

    assert_track_links(&pack_dir, 3);
}

// ── 测试 4: 嵌套目录 fold ──

#[test]
fn nested_directories() {
    let env = TestEnv::new();
    let (pack_dir, target_dir, cfg) = setup_pack(
        &env,
        "e2e-nested",
        &symlink_config(&env.config_path().join("e2e-nested").to_string_lossy()),
        &[
            ("a/b/c/d/file.txt", "deep\n"),
            ("a/b/c/d/e/leaf.txt", "leaf\n"),
        ],
    );

    install(&cfg, &pack_dir, false).expect("install");

    let link_dir = target_dir.join("a");
    assert_symlink(&link_dir, pack_dir.join("a"));

    let deep_path = target_dir.join("a/b/c/d/file.txt");
    assert!(deep_path.try_exists().unwrap());
    let content = std::fs::read_to_string(&deep_path).unwrap();
    assert_eq!(content, "deep\n");

    let leaf_path = target_dir.join("a/b/c/d/e/leaf.txt");
    assert!(leaf_path.try_exists().unwrap());

    let track_path = assert_track_links(&pack_dir, 1);

    clean(&cfg, &pack_dir, false).expect("clean");
    assert_not_exists(&link_dir);
    assert_not_exists(&track_path);
}

// ── 测试 5: 多 pack 独立管理 ──

#[test]
fn multiple_packs() {
    let env = TestEnv::new();
    let target_a = env.config_path().join("target-a");
    let target_b = env.config_path().join("target-b");
    std::fs::create_dir_all(&target_a).unwrap();
    std::fs::create_dir_all(&target_b).unwrap();

    let pack_base = env.state_path().join("packs");
    std::fs::create_dir_all(&pack_base).unwrap();

    let pack_a_dir = pack_base.join("pack-a");
    std::fs::create_dir_all(&pack_a_dir).unwrap();
    std::fs::write(
        pack_a_dir.join("stow-cm.toml"),
        symlink_config(&target_a.to_string_lossy()),
    )
    .unwrap();
    common::write_pack_file(&pack_a_dir, "apple.txt", "apple\n");
    common::write_pack_file(&pack_a_dir, "animal.txt", "animal\n");

    let pack_b_dir = pack_base.join("pack-b");
    std::fs::create_dir_all(&pack_b_dir).unwrap();
    std::fs::write(
        pack_b_dir.join("stow-cm.toml"),
        symlink_config(&target_b.to_string_lossy()),
    )
    .unwrap();
    common::write_pack_file(&pack_b_dir, "banana.txt", "banana\n");
    common::write_pack_file(&pack_b_dir, "bridge.txt", "bridge\n");

    let global = Arc::new(Config::global().expect("global"));
    let pack_a_dir = std::fs::canonicalize(&pack_a_dir).expect("canonicalize a");
    let pack_b_dir = std::fs::canonicalize(&pack_b_dir).expect("canonicalize b");
    let cfg_a = for_pack(&pack_a_dir, &global);
    let cfg_b = for_pack(&pack_b_dir, &global);

    install(&cfg_a, &pack_a_dir, false).expect("install a");
    install(&cfg_b, &pack_b_dir, false).expect("install b");

    let link_apple = target_a.join("apple.txt");
    let link_animal = target_a.join("animal.txt");
    let link_banana = target_b.join("banana.txt");
    let link_bridge = target_b.join("bridge.txt");

    assert_symlink(&link_apple, pack_a_dir.join("apple.txt"));
    assert_symlink(&link_animal, pack_a_dir.join("animal.txt"));
    assert_symlink(&link_banana, pack_b_dir.join("banana.txt"));
    assert_symlink(&link_bridge, pack_b_dir.join("bridge.txt"));

    assert_track_links(&pack_a_dir, 2);
    assert_track_links(&pack_b_dir, 2);

    remove(&cfg_a, &pack_a_dir, false).expect("remove a");

    assert_not_exists(&link_apple);
    assert_not_exists(&link_animal);
    assert_symlink(&link_banana, pack_b_dir.join("banana.txt"));
    assert_symlink(&link_bridge, pack_b_dir.join("bridge.txt"));

    let track_a = stow_cm::command::resolve_track_file(&pack_a_dir).unwrap();
    assert_not_exists(&track_a);
    assert_track_links(&pack_b_dir, 2);
}
