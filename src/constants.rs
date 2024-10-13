use const_format::formatcp;

// ----------------------------------------------------------------------
//    - inner -
// ----------------------------------------------------------------------

pub(crate) const PACK_NAME_ENV: &str = "PACK_NAME";

pub(crate) const CONFIG_FILE_NAME: &str = "stow-cm.toml";

pub(crate) const PACK_STATE_HOME: &str = formatcp!("${{XDG_STATE_HOME:-~/.local/state}}/stow-cm/${{{}}}", PACK_NAME_ENV);

pub(crate) const PACK_TRACK_FILE: &str = formatcp!("${{{}}}/track.toml", PACK_STATE_HOME);

/// if the value of Some(value) is !, it is equivalent to None.
pub(crate) const UNSET_VALUE: &str = "!";

// ----------------------------------------------------------------------
//    - global -
// ----------------------------------------------------------------------

pub(crate) const GLOBAL_CONFIG_FILE: &str = "${XDG_CONFIG_HOME:-~/.config}/stow-cm/config.toml";

// ----------------------------------------------------------------------
//    - default -
// ----------------------------------------------------------------------

pub(crate) const DEFAULT_PACK_DECRYPT: &str = formatcp!("${{{}}}/decrypted/", PACK_STATE_HOME);

pub(crate) const DEFAULT_PACK_TARGET: &str =
    formatcp!("${{XDG_CONFIG_HOME:-~/.config}}/${{{}}}/", PACK_NAME_ENV);

pub(crate) const DEFAULT_DECRYPT_LEFT_BOUNDARY: &str = "&{";

pub(crate) const DEFAULT_DECRYPT_RIGHT_BOUNDARY: &str = "}";

pub(crate) const DEFAULT_CRYPT_ALG: &str = "ChaCha20-Poly1305";
