#![allow(dead_code)]

use anyhow::{anyhow, bail};
use log::debug;
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
    unwrap: bool,
) -> Result<String> {
    let mut r = String::new();
    let mut last_index = 0;
    while let Some(li) = content[last_index..].find(left_boundary) {
        let content = &content[last_index..];
        r.push_str(&content[..li]);
        let content = &content[li..];
        if let Some(ei) = content.find(right_boundary) {
            let dec_content = decrypt(&content[left_boundary.len()..ei], alg_name, key)?;
            if !unwrap {
                r.push_str(left_boundary);
            }
            r.push_str(&dec_content);
            if !unwrap {
                r.push_str(right_boundary);
            }
            last_index = last_index + li + ei + right_boundary.len();
        }
    }
    r.push_str(&content[last_index..]);
    Ok(r)
}

/// decrypt content
/// return format: <enc_content_base64>:<nonce_base64>
pub(crate) fn encrypt(content: &str, alg_name: &str, key: &[u8]) -> Result<String> {
    let mut nonce_value = [0_u8; NONCE_LEN];
    SystemRandom::new().fill(&mut nonce_value)?;
    let nonce = Nonce::try_assume_unique_for_key(&nonce_value)?;

    let mut content = content.as_bytes().to_vec();

    let unbound_key = UnboundKey::new(algorithm(alg_name)?, key)?;
    LessSafeKey::new(unbound_key).seal_in_place_append_tag(nonce, Aad::empty(), &mut content)?;

    let enc_content_base64 = base64::encode(&content)?;
    let nonce_base64 = base64::encode(&nonce_value)?;
    Ok(format!("{}:{}", enc_content_base64, nonce_base64))
}

/// decrypt content
pub(crate) fn decrypt(content: &str, alg_name: &str, key: &[u8]) -> Result<String> {
    let splitn: Vec<_> = content.splitn(2, ':').collect();

    let [encrypted_content_base64, nonce_base64] = match splitn[..] {
        [a, b] => Ok([a, b]),
        _ => Err(anyhow!(
            r#"encryption markers do not contain nonce information
        in the format of encrypt_data_base64:nonce_base64
        content: {}
        "#,
            content
        )),
    }?;
    debug!(
        "encrypted_content={}, nonce={}",
        encrypted_content_base64, nonce_base64
    );
    let mut encrypted_content = base64::decode(encrypted_content_base64)?;
    let nonce = base64::decode(nonce_base64)?;

    let unbound_key = UnboundKey::new(algorithm(alg_name)?, key)?;
    let origin_data = LessSafeKey::new(unbound_key).open_in_place(
        Nonce::try_assume_unique_for_key(&nonce)?,
        Aad::empty(),
        &mut encrypted_content,
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

// demo use of opening_key and sealing_key: https://web3developer.io/authenticated-encryption-in-rust-using-ring/
// let mut opening_key = OpeningKey::new(unbound_key, OneShotNonceSeq(&nonce));
// let origin_data = opening_key.open_in_place(Aad::empty(), encrypted_content.as_mut_slice())?;

// struct OneShotNonceSeq<'s>(&'s [u8]);

// impl<'s> NonceSequence for OneShotNonceSeq<'s> {
//     fn advance(&mut self) -> std::result::Result<Nonce, ring::error::Unspecified> {
//         Nonce::try_assume_unique_for_key(&self.0)
//     }
// }

#[cfg(test)]
mod test {

    use crate::base64;
    use crate::error::Result;

    #[test]
    fn encrypt_test() -> Result<()> {
        // let plain_text = "Hello world!";
        // let alg_name = "AES-256-GCM";
        // let key_base64 = "Irl9RrW55AXkbPwBxI85/33pDNyte6753h/G1YERblo=";
        // let key = base64::decode(key_base64)?;
        // let nonce_base64 = "U5mgpHMN5h9EYvH2";
        // let nonce = base64::decode(nonce_base64)?;

        // let encrypted_text = super::encrypt_with_nonce(plain_text, alg_name, &key, &nonce)?;

        // println!("{}", encrypted_text);

        Ok(())
    }

    #[test]
    fn decrypt_test() -> Result<()> {
        let plain_text = "Hello world!";
        let alg_name = "AES-256-GCM";
        let key_base64 = "Irl9RrW55AXkbPwBxI85/33pDNyte6753h/G1YERblo=";
        let key = base64::decode(key_base64)?;
        let nonce_base64 = "U5mgpHMN5h9EYvH2";
        // let nonce = base64::decode(nonce_base64)?;

        let encrypted_text = "sPO5zRwCrZG0J834t/sd/eeB9F2VthSwrnzLAw==";

        let origin_text = super::decrypt(
            &format!("{}:{}", encrypted_text, nonce_base64),
            alg_name,
            &key,
        )?;

        assert_eq!(plain_text, origin_text);
        Ok(())
    }
}
