use std::path::PathBuf;

fn config_home() -> PathBuf {
    dirs::config_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn state_home() -> PathBuf {
    dirs::state_dir().unwrap_or_default()
}

/// 全局 XDG 用户配置文件完整路径（运行时解析）
pub fn global_xdg_config_path() -> PathBuf {
    config_home().join("stow-cm").join("config.toml")
}

/// 全局系统配置文件路径（运行时解析）
pub fn global_config_path() -> PathBuf {
    PathBuf::from("/etc/stow-cm/config.toml")
}

/// pack 状态目录模板，含 `${PACK_ID}` 占位符
pub fn pack_state_home() -> String {
    format!("{}/stow-cm/${{PACK_ID}}", state_home().display())
}

/// pack track 文件路径模板，含 `${PACK_ID}` 占位符
pub fn pack_track_file() -> String {
    let state_home = pack_state_home();
    format!("{state_home}/track.toml")
}

/// pack 解密文件目录模板，含 `${PACK_ID}` 占位符
pub fn default_pack_decrypt() -> String {
    let state_home = pack_state_home();
    format!("{state_home}/decrypted/")
}

/// pack 安装目标目录模板，含 `${PACK_NAME}` 占位符
pub fn default_pack_target() -> String {
    format!("{}/${{PACK_NAME}}/", config_home().display())
}
