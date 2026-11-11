//! Tokens, password hashing and the secret box.

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Context, Result, anyhow, bail};
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use rand::RngCore;
use sha2::{Digest, Sha256};

/// A new random bearer token with a recognisable prefix (`nmu_`, `nmk_`,
/// `nmr_`, `nmi_`). Only its hash is ever stored.
pub fn new_token(prefix: &str) -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("{prefix}_{}", URL_SAFE_NO_PAD.encode(bytes))
}

pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

pub fn hash_password(password: &str) -> Result<String> {
    let mut salt_bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt_bytes);
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|e| anyhow!("salt: {e}"))?;
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow!("could not hash password: {e}"))?
        .to_string())
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash)
        .map(|parsed| Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok())
        .unwrap_or(false)
}

/// Encrypts integration and harness credentials before they reach Postgres.
#[derive(Clone)]
pub struct SecretBox {
    cipher: Aes256Gcm,
}

impl SecretBox {
    /// `key` is 32 bytes, base64 encoded (`hyperbeam-server gen-key`).
    pub fn from_base64(key: &str) -> Result<Self> {
        let bytes = STANDARD
            .decode(key.trim())
            .or_else(|_| URL_SAFE_NO_PAD.decode(key.trim()))
            .context("the master key is not valid base64")?;
        if bytes.len() != 32 {
            bail!("the master key must decode to 32 bytes, got {}", bytes.len());
        }
        Ok(Self { cipher: Aes256Gcm::new_from_slice(&bytes).map_err(|e| anyhow!("{e}"))? })
    }

    pub fn generate_key() -> String {
        let mut bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut bytes);
        STANDARD.encode(bytes)
    }

    /// Returns `(nonce, ciphertext)`. `context` binds the ciphertext to the
    /// row it is stored under, so secrets cannot be swapped between keys.
    pub fn seal(&self, context: &str, plaintext: &str) -> Result<(Vec<u8>, Vec<u8>)> {
        let mut nonce = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce);
        let payload = aes_gcm::aead::Payload { msg: plaintext.as_bytes(), aad: context.as_bytes() };
        let ciphertext = self
            .cipher
            .encrypt(Nonce::from_slice(&nonce), payload)
            .map_err(|_| anyhow!("encryption failed"))?;
        Ok((nonce.to_vec(), ciphertext))
    }

    pub fn open(&self, context: &str, nonce: &[u8], ciphertext: &[u8]) -> Result<String> {
        if nonce.len() != 12 {
            bail!("stored secret has a malformed nonce");
        }
        let payload = aes_gcm::aead::Payload { msg: ciphertext, aad: context.as_bytes() };
        let plain = self
            .cipher
            .decrypt(Nonce::from_slice(nonce), payload)
            .map_err(|_| anyhow!("could not decrypt secret `{context}` (was the master key changed?)"))?;
        String::from_utf8(plain).context("stored secret is not UTF-8")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_unique_and_hash_stably() {
        let a = new_token("nmr");
        let b = new_token("nmr");
        assert!(a.starts_with("nmr_") && a != b);
        assert_eq!(hash_token(&a), hash_token(&a));
        assert_ne!(hash_token(&a), hash_token(&b));
    }

    #[test]
    fn passwords_verify() {
        let hash = hash_password("correct horse").unwrap();
        assert!(verify_password("correct horse", &hash));
        assert!(!verify_password("wrong", &hash));
        assert!(!verify_password("x", "not-a-hash"));
    }

    #[test]
    fn secrets_round_trip_and_are_bound_to_their_key() {
        let sb = SecretBox::from_base64(&SecretBox::generate_key()).unwrap();
        let (nonce, ct) = sb.seal("integration/github", "ghp_secret").unwrap();
        assert_eq!(sb.open("integration/github", &nonce, &ct).unwrap(), "ghp_secret");
        assert!(sb.open("integration/linear", &nonce, &ct).is_err());
        let other = SecretBox::from_base64(&SecretBox::generate_key()).unwrap();
        assert!(other.open("integration/github", &nonce, &ct).is_err());
        assert!(SecretBox::from_base64("c2hvcnQ=").is_err());
    }
}
