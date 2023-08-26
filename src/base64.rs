#![allow(dead_code)]

use std::io::{Cursor, Read, Write};

use base64::engine::general_purpose;

use crate::error::Result;

pub(crate) fn decode(data: &str) -> Result<Vec<u8>> {
    let mut reader =
        base64::read::DecoderReader::new(Cursor::new(data.as_bytes()), &general_purpose::STANDARD);
    let mut buf = Vec::<u8>::new();
    let _ = reader.read_to_end(&mut buf)?;
    Ok(buf)
}

pub(crate) fn encode(data: &[u8]) -> Result<String> {
    let mut enc = base64::write::EncoderStringWriter::new(&general_purpose::STANDARD);
    enc.write_all(data)?;
    Ok(enc.into_inner())
}
