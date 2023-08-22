use const_format::formatcp;


// ----------------------------------------------------------------------
//    - inner -
// ----------------------------------------------------------------------

pub(crate) const PACK_NAME_ENV: &str = "PACK_NAME";

pub(crate) const CONFIG_FILE_NAME: &str = "stow-cm.toml";

pub(crate) const PACK_TRACK_FILE: &str = formatcp!(".stow-cm-track/${{{}}}.toml", PACK_NAME_ENV);


// ----------------------------------------------------------------------
//    - global -
// ----------------------------------------------------------------------

pub(crate) const GLOBAL_CONFIG_FILE: &str = "${XDG_CONFIG_HOME:-~/.config}/stow-cm/config.toml";


// ----------------------------------------------------------------------
//    - default -
// ----------------------------------------------------------------------

pub(crate) const DEFAULT_PACK_DECRYPT: &str = formatcp!("${{XDG_STATE_HOME:-~/.local/state}}/stow-cm/${{{}}}/", PACK_NAME_ENV);

pub(crate) const DEFAULT_PACK_TARGET: &str = formatcp!("${{XDG_CONFIG_HOME:-~/.config}}/${{{}}}/", PACK_NAME_ENV);
