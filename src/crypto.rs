#![allow(dead_code)]

use anyhow::{anyhow, bail};
use ring::aead::{
    Aad, Algorithm, LessSafeKey, Nonce, UnboundKey, AES_128_GCM, AES_256_GCM, CHACHA20_POLY1305,
    NONCE_LEN,
};
use ring::rand::SecureRandom;
use ring::rand::SystemRandom;

use crate::base64;
use crate::error::Result;

/// decrypt content
pub(crate) fn encrypt_inline(
    content: &str,
    alg_name: &str,
    key: &[u8],
    left_boundary: &str,
    right_boundary: &str,
) -> Result<String> {
    let mut r = String::new();
    let mut last_index = 0;
    while let Some(li) = content.find(left_boundary) {
        r.push_str(&content[last_index..li]);
        last_index = li;
        if let Some(ei) = content[li..].find(right_boundary) {
            let enc_content = encrypt(&content[(li + left_boundary.len())..ei], alg_name, key)?;
            r.push_str(left_boundary);
            r.push_str(&enc_content);
            r.push_str(right_boundary);
            last_index = ei + right_boundary.len();
        }
    }
    r.push_str(&content[last_index..]);
    Ok(r)
}

/// decrypt content
pub(crate) fn decrypt_inline(
    content: &str,
    alg_name: &str,
    key: &[u8],
    left_boundary: &str,
    right_boundary: &str,
) -> Result<String> {
    let mut r = String::new();
    let mut last_index = 0;
    while let Some(li) = content.find(left_boundary) {
        r.push_str(&content[last_index..li]);
        last_index = li;
        if let Some(ei) = content[li..].find(right_boundary) {
            let dec_content = decrypt(&content[(li + left_boundary.len())..ei], alg_name, key)?;
            r.push_str(left_boundary);
            r.push_str(&dec_content);
            r.push_str(right_boundary);
            last_index = ei + right_boundary.len();
        }
    }
    r.push_str(&content[last_index..]);
    Ok(r)
}

/// decrypt content
pub(crate) fn encrypt(content: &str, alg_name: &str, key: &[u8]) -> Result<String> {
    let mut random = [0_u8; NONCE_LEN];
    SystemRandom::new().fill(&mut random)?;
    let nonce = Nonce::try_assume_unique_for_key(&random)?;

    let mut content = content.as_bytes().to_vec();

    let unbound_key = UnboundKey::new(algorithm(alg_name)?, key)?;
    let _ = LessSafeKey::new(unbound_key).seal_in_place_separate_tag(
        nonce,
        Aad::empty(),
        &mut content,
    )?;

    let enc_data = String::from_utf8(content)?;
    Ok(enc_data)
}

/// decrypt content
pub(crate) fn decrypt(content: &str, alg_name: &str, key: &[u8]) -> Result<String> {
    let splitn: Vec<_> = content.splitn(2, ':').collect();

    let [msg, nonce] = match splitn[..] {
        [a, b] => Ok([a, b]),
        _ => Err(anyhow!(
            r#"encryption markers do not contain nonce information in the format of encrypt_data:nonce
        content: {}
        "#,
            content
        )),
    }?;
    let mut msg = base64::decode(msg)?;
    let nonce = base64::decode(nonce)?;

    let unbound_key = UnboundKey::new(algorithm(alg_name)?, key)?;
    let origin_data = LessSafeKey::new(unbound_key).open_in_place(
        Nonce::try_assume_unique_for_key(&nonce)?,
        Aad::empty(),
        &mut msg,
    )?;
    let origin_data = String::from_utf8(origin_data.to_vec())?;
    Ok(origin_data)
}

/// convert algorithmName to algorithm
fn algorithm(alg_name: &str) -> Result<&'static Algorithm> {
    match alg_name {
        "AES-128-GCM" => Ok(&AES_128_GCM),
        "AES-256-GCM" => Ok(&AES_256_GCM),
        "ChaCha20-Poly1305" => Ok(&CHACHA20_POLY1305),
        _ => bail!(""),
    }
}
