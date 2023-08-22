use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::symlink::Symlink;

/// track struct
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Track {
    /// save links
    pub links: Vec<Symlink>,
    /// decrypted file path
    pub decrypted_path: Option<PathBuf>,
}
