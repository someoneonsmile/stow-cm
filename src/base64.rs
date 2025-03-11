#![allow(dead_code)]

use anyhow::{Context, anyhow};
use base64::{Engine, engine::general_purpose};
use lazy_regex::{Lazy, Regex, lazy_regex};
use log::debug;

use crate::error::Result;

static BLANK_REPLACER: Lazy<Regex> = lazy_regex!(r"[\s|\n]*");

pub(crate) fn decode(data: &str) -> Result<Vec<u8>> {
    // debug!("decode: {:?}", data);
    let data_replace = BLANK_REPLACER.replace_all(data, "");
    debug!("decode: data={:?}, replace={:?}", data, data_replace);
    general_purpose::STANDARD
        .decode(data_replace.as_bytes())
        .with_context(|| anyhow!("base64 decode error, content={data}"))
}

pub(crate) fn encode(data: &[u8]) -> String {
    debug!("encode: {:?}", data);
    general_purpose::STANDARD.encode(data)
}

#[cfg(test)]
mod test {

    use super::*;

    /// encode
    #[test]
    fn encode_test() {
        let b = vec![
            0xa0, 0xff, 0xf5, 0x5d, 0x29, 0xc3, 0xb2, 0xc9, 0x02, 0x2b, 0xa3, 0x74,
        ];
        let e = super::encode(&b);
        println!("{e:?}");
    }

    /// decode
    #[test]
    fn decode_test() {
        let s = "sPO5zRwO+ompOcW9hew=";
        let d = decode(s);
        println!("{d:?}");
        assert!(d.is_ok());

        let s = "U5mgpHMN5h9EYvH2";
        let d = decode(s);
        println!("{d:?}");
        assert!(d.is_ok());

        let s = "oP/1XSnDsskCK6N0";
        let d = decode(s);
        println!("{d:?}");
        assert!(d.is_ok());

        let s = "0wUHEBS3RtDjTK+L";
        let d = decode(s);
        println!("{d:?}");
        assert!(d.is_ok());
    }
}
