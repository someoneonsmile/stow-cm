use const_format::formatcp;

// ----------------------------------------------------------------------
//    - inner -
// ----------------------------------------------------------------------

pub const PACK_ID_ENV: &str = "PACK_ID";

pub const PACK_NAME_ENV: &str = "PACK_NAME";

pub const CONFIG_FILE_NAME: &str = "stow-cm.toml";

pub const PACK_STATE_HOME: &str = formatcp!(
    "${{XDG_STATE_HOME:-~/.local/state}}/stow-cm/${{{}}}",
    PACK_ID_ENV
);

pub const PACK_TRACK_FILE: &str = formatcp!("{}/track.toml", PACK_STATE_HOME);

/// if the value of Some(value) is !, it is equivalent to None.
pub const UNSET_VALUE: &str = "!";

// ----------------------------------------------------------------------
//    - global -
// ----------------------------------------------------------------------

pub const GLOBAL_XDG_CONFIG_FILE: &str = "${XDG_CONFIG_HOME:-~/.config}/stow-cm/config.toml";
pub const GLOBAL_CONFIG_FILE: &str = "/etc/stow-cm/config.toml";

// ----------------------------------------------------------------------
//    - default -
// ----------------------------------------------------------------------

pub const DEFAULT_PACK_DECRYPT: &str = formatcp!("{}/decrypted/", PACK_STATE_HOME);

pub const DEFAULT_PACK_TARGET: &str =
    formatcp!("${{XDG_CONFIG_HOME:-~/.config}}/${{{}}}/", PACK_NAME_ENV);

pub const DEFAULT_DECRYPT_LEFT_BOUNDARY: &str = "&{";

pub const DEFAULT_DECRYPT_RIGHT_BOUNDARY: &str = "}";

pub const DEFAULT_CRYPT_ALG: &str = "ChaCha20-Poly1305";
