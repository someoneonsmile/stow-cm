#![allow(clippy::unwrap_used)]
#![allow(clippy::expect_used)]

mod common;

use std::sync::Arc;

use common::{TestEnv, assert_exists, assert_not_exists, assert_symlink, assert_track_links};
use stow_cm::command::{clean, install, resolve_track_file};
use stow_cm::config::Config;

/// 辅助：创建 pack（放在 state 目录下）和 target（放在 config 目录下），两者分离。
fn setup_separated_pack(
    env: &TestEnv,
    pack_name: &str,
) -> (std::path::PathBuf, std::path::PathBuf) {
    let target_dir = env.config_path().join(pack_name);
    std::fs::create_dir_all(&target_dir).expect("create target dir");

    // pack 放在 state 目录下，不与 target 冲突
    let pack_base = env.state_path().join("packs");
    std::fs::create_dir_all(&pack_base).expect("create pack base");
    let pack_dir = pack_base.join(pack_name);
    std::fs::create_dir_all(&pack_dir).expect("create pack dir");

    let config_toml = format!(
        r#"target = '{}'
mode = 'symlink'
"#,
        target_dir.to_string_lossy()
    );
    std::fs::write(pack_dir.join("stow-cm.toml"), config_toml).expect("write config");

    common::write_pack_file(&pack_dir, "file_a.txt", "content a\n");
    common::write_pack_file(&pack_dir, "sub/file_b.txt", "content b\n");

    (pack_dir, target_dir)
}

#[test]
fn test_clean_basic() {
    let env = TestEnv::new();
    let global = Arc::new(Config::global().expect("global config"));
    let (pack_dir, target_dir) = setup_separated_pack(&env, "test-clean-basic");

    let config = Arc::new(Config::for_pack(&pack_dir, &global, None, false).expect("for_pack"));

    install(&config, &pack_dir, false).expect("install");

    // fold 默认启用：sub/ 被折叠为目录级 symlink
    let link_a = target_dir.join("file_a.txt");
    let link_sub = target_dir.join("sub");
    assert_symlink(&link_a, &pack_dir.join("file_a.txt"));
    assert_symlink(&link_sub, &pack_dir.join("sub"));
    // sub/file_b.txt 通过目录 symlink 可达
    assert_exists(&target_dir.join("sub").join("file_b.txt"));

    let pack_name = config.resolve_pack_name(&pack_dir).expect("pack name");
    let track_path = assert_track_links(&pack_dir, &pack_name, 2);

    clean(&config, &pack_dir, false).expect("clean");

    assert_not_exists(&link_a);
    assert_not_exists(&link_sub);
    assert_not_exists(&track_path);
}

#[test]
fn test_clean_empty() {
    let env = TestEnv::new();
    let global = Arc::new(Config::global().expect("global config"));
    let (pack_dir, target_dir) = setup_separated_pack(&env, "test-clean-empty");

    let config = Arc::new(Config::for_pack(&pack_dir, &global, None, false).expect("for_pack"));

    // 从未安装，直接 clean
    clean(&config, &pack_dir, false).expect("clean should be no-op");

    // 验证 target 下无任何链接
    assert_not_exists(&target_dir.join("file_a.txt"));
    assert_not_exists(&target_dir.join("sub").join("file_b.txt"));

    // track file 也不存在
    let pack_name = config.resolve_pack_name(&pack_dir).expect("pack name");
    let track_path = resolve_track_file(&pack_dir, &pack_name).expect("resolve track file");
    assert_not_exists(&track_path);
}

#[test]
fn test_clean_dry_run() {
    let env = TestEnv::new();
    let global = Arc::new(Config::global().expect("global config"));
    let (pack_dir, target_dir) = setup_separated_pack(&env, "test-clean-dry-run");

    let config = Arc::new(Config::for_pack(&pack_dir, &global, None, false).expect("for_pack"));

    install(&config, &pack_dir, false).expect("install");

    let link_a = target_dir.join("file_a.txt");
    let link_b = target_dir.join("sub").join("file_b.txt");
    assert_exists(&link_a);
    assert_exists(&link_b);

    let pack_name = config.resolve_pack_name(&pack_dir).expect("pack name");
    let track_path = assert_track_links(&pack_dir, &pack_name, 2);

    clean(&config, &pack_dir, true).expect("dry-run clean");

    assert_exists(&link_a);
    assert_exists(&link_b);
    assert_exists(&track_path);
}

#[test]
fn test_clean_after_manual_link_removal() {
    let env = TestEnv::new();
    let global = Arc::new(Config::global().expect("global config"));
    let (pack_dir, target_dir) = setup_separated_pack(&env, "test-clean-manual-rm");

    let config = Arc::new(Config::for_pack(&pack_dir, &global, None, false).expect("for_pack"));

    install(&config, &pack_dir, false).expect("install");

    let link_a = target_dir.join("file_a.txt");
    let link_sub = target_dir.join("sub");
    assert_symlink(&link_a, &pack_dir.join("file_a.txt"));
    assert_symlink(&link_sub, &pack_dir.join("sub"));

    // 手动删除 link_a，保留 sub 目录
    std::fs::remove_file(&link_a).expect("manual remove");
    assert_not_exists(&link_a);
    assert_exists(&link_sub);

    let pack_name = config.resolve_pack_name(&pack_dir).expect("pack name");
    let track_path = assert_track_links(&pack_dir, &pack_name, 2);

    // clean 通过文件系统扫描，仍能找到 sub 目录 symlink
    clean(&config, &pack_dir, false).expect("clean");

    assert_not_exists(&link_sub);
    assert_not_exists(&track_path);
}

#[test]
fn test_clean_target_only_pack() {
    let env = TestEnv::new();
    let global = Arc::new(Config::global().expect("global config"));

    let target_dir = env.config_path().join("shared-target");
    std::fs::create_dir_all(&target_dir).expect("create shared target");

    // pack_a 放在 state 下
    let pack_base = env.state_path().join("packs");
    std::fs::create_dir_all(&pack_base).expect("create pack base");

    let config_a = format!(
        r#"target = '{}'
mode = 'symlink'
"#,
        target_dir.to_string_lossy()
    );
    let pack_a_dir = pack_base.join("pack-a");
    std::fs::create_dir_all(&pack_a_dir).expect("create pack a");
    std::fs::write(pack_a_dir.join("stow-cm.toml"), &config_a).expect("write config a");
    common::write_pack_file(&pack_a_dir, "file_a.txt", "content a\n");

    let config_b = format!(
        r#"target = '{}'
mode = 'symlink'
"#,
        target_dir.to_string_lossy()
    );
    let pack_b_dir = pack_base.join("pack-b");
    std::fs::create_dir_all(&pack_b_dir).expect("create pack b");
    std::fs::write(pack_b_dir.join("stow-cm.toml"), &config_b).expect("write config b");
    common::write_pack_file(&pack_b_dir, "file_b.txt", "content b\n");

    let cfg_a = Arc::new(Config::for_pack(&pack_a_dir, &global, None, false).expect("for_pack a"));
    let cfg_b = Arc::new(Config::for_pack(&pack_b_dir, &global, None, false).expect("for_pack b"));

    install(&cfg_a, &pack_a_dir, false).expect("install a");
    install(&cfg_b, &pack_b_dir, false).expect("install b");

    let link_a = target_dir.join("file_a.txt");
    let link_b = target_dir.join("file_b.txt");
    assert_symlink(&link_a, &pack_a_dir.join("file_a.txt"));
    assert_symlink(&link_b, &pack_b_dir.join("file_b.txt"));

    clean(&cfg_a, &pack_a_dir, false).expect("clean a");

    assert_not_exists(&link_a);
    assert_symlink(&link_b, &pack_b_dir.join("file_b.txt"));

    let name_a = cfg_a.resolve_pack_name(&pack_a_dir).expect("pack name a");
    let track_a = resolve_track_file(&pack_a_dir, &name_a).expect("track a");
    assert_not_exists(&track_a);

    let name_b = cfg_b.resolve_pack_name(&pack_b_dir).expect("pack name b");
    let _track_b = assert_track_links(&pack_b_dir, &name_b, 1);
}
