use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit, Payload},
};
use anyhow::{Context, Result, anyhow, bail};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD as B64_STD, URL_SAFE, URL_SAFE_NO_PAD},
};
use rand::{RngCore, rngs::OsRng};

const CIPHER_PREFIX: &str = "enc:v1:";
const NONCE_LEN: usize = 12;

/// AES-256-GCM cipher for encrypting sensitive user secrets (such as WaniKani API tokens) at rest.
#[derive(Clone)]
pub struct TokenCipher {
    key: [u8; 32],
}

impl TokenCipher {
    /// Creates a cipher from a raw 32-byte key.
    pub fn from_bytes(key: [u8; 32]) -> Self {
        Self { key }
    }

    /// Parses a 32-byte key from either 64 hex characters or base64 (standard or URL-safe).
    pub fn from_key_str(raw: &str) -> Result<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            bail!("ENCRYPTION_KEY must not be empty");
        }

        let decoded = if trimmed.len() == 64 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
            hex::decode(trimmed).context("Invalid hex in ENCRYPTION_KEY")?
        } else {
            B64_STD
                .decode(trimmed)
                .or_else(|_| URL_SAFE.decode(trimmed))
                .or_else(|_| URL_SAFE_NO_PAD.decode(trimmed))
                .map_err(|_| {
                    anyhow!(
                        "ENCRYPTION_KEY must be 64 hex characters or a base64-encoded 32-byte key"
                    )
                })?
        };

        let key: [u8; 32] = decoded.try_into().map_err(|v: Vec<u8>| {
            anyhow!(
                "ENCRYPTION_KEY must decode to exactly 32 bytes (256 bits), got {} bytes",
                v.len()
            )
        })?;

        Ok(Self { key })
    }

    /// Loads `ENCRYPTION_KEY` from the environment.
    ///
    /// In production (`APP_ENV=production` or `RUST_ENV=production`), a missing `ENCRYPTION_KEY`
    /// is a fatal startup error. In local development, an ephemeral random key is generated with
    /// a prominent warning so no hardcoded key is ever embedded in source code.
    pub fn from_env() -> Result<Self> {
        let is_production = std::env::var("APP_ENV")
            .or_else(|_| std::env::var("RUST_ENV"))
            .map(|v| v.eq_ignore_ascii_case("production"))
            .unwrap_or(false);

        match std::env::var("ENCRYPTION_KEY") {
            Ok(raw) if !raw.trim().is_empty() => Self::from_key_str(&raw),
            _ if is_production => {
                bail!(
                    "ENCRYPTION_KEY is required in production (provide 64 hex chars or 32-byte base64)"
                );
            }
            _ => {
                tracing::warn!(
                    "ENCRYPTION_KEY is not set; generating an ephemeral AES-256-GCM key for this process. \
                     Set ENCRYPTION_KEY (e.g. `openssl rand -hex 32`) so encrypted WaniKani tokens persist across restarts."
                );
                Ok(Self::generate_ephemeral())
            }
        }
    }

    /// Generates a fresh random 256-bit key using OS entropy.
    pub fn generate_ephemeral() -> Self {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        Self { key }
    }

    fn aad_for_user(user_id: i64) -> String {
        format!("wk_profile:api_token:v1:user:{}", user_id)
    }

    /// Encrypts a WaniKani API token bound to `user_id` via AES-256-GCM associated data.
    pub fn encrypt_for_user(&self, user_id: i64, plaintext: &str) -> Result<String> {
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| anyhow!("Failed to initialize AES-256-GCM cipher: {}", e))?;

        let mut nonce_bytes = [0u8; NONCE_LEN];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let aad = Self::aad_for_user(user_id);

        let ciphertext = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: plaintext.as_bytes(),
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| anyhow!("Failed to encrypt API token"))?;

        let mut combined = Vec::with_capacity(NONCE_LEN + ciphertext.len());
        combined.extend_from_slice(&nonce_bytes);
        combined.extend_from_slice(&ciphertext);

        Ok(format!("{}{}", CIPHER_PREFIX, B64_STD.encode(combined)))
    }

    /// Decrypts a stored `enc:v1:...` token, verifying integrity and user binding.
    pub fn decrypt_for_user(&self, user_id: i64, stored: &str) -> Result<String> {
        let Some(encoded) = stored.strip_prefix(CIPHER_PREFIX) else {
            bail!("Stored API token does not use expected encrypted envelope format");
        };

        let combined = B64_STD
            .decode(encoded)
            .context("Failed to base64-decode encrypted API token")?;
        if combined.len() <= NONCE_LEN {
            bail!("Encrypted API token payload is truncated");
        }

        let (nonce_bytes, ciphertext) = combined.split_at(NONCE_LEN);
        let cipher = Aes256Gcm::new_from_slice(&self.key)
            .map_err(|e| anyhow!("Failed to initialize AES-256-GCM cipher: {}", e))?;
        let nonce = Nonce::from_slice(nonce_bytes);
        let aad = Self::aad_for_user(user_id);

        let plaintext = cipher
            .decrypt(
                nonce,
                Payload {
                    msg: ciphertext,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| anyhow!("Failed to decrypt API token (invalid key or tampered data)"))?;

        String::from_utf8(plaintext).context("Decrypted API token is not valid UTF-8")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_encrypt_decrypt_hex_key() {
        let hex_key = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let cipher = TokenCipher::from_key_str(hex_key).unwrap();
        let token = "wk-secret-token-12345";

        let enc1 = cipher.encrypt_for_user(42, token).unwrap();
        let enc2 = cipher.encrypt_for_user(42, token).unwrap();

        assert!(enc1.starts_with("enc:v1:"));
        assert!(!enc1.contains(token));
        assert_ne!(enc1, enc2, "fresh random nonces must yield distinct ciphertexts");

        let dec = cipher.decrypt_for_user(42, &enc1).unwrap();
        assert_eq!(dec, token);
    }

    #[test]
    fn roundtrip_base64_key() {
        let raw = [7u8; 32];
        let b64 = B64_STD.encode(raw);
        let cipher = TokenCipher::from_key_str(&b64).unwrap();
        let enc = cipher.encrypt_for_user(1, "token-abc").unwrap();
        assert_eq!(cipher.decrypt_for_user(1, &enc).unwrap(), "token-abc");
    }

    #[test]
    fn rejects_wrong_user_aad_and_wrong_key() {
        let cipher_a = TokenCipher::generate_ephemeral();
        let cipher_b = TokenCipher::generate_ephemeral();
        let enc = cipher_a.encrypt_for_user(10, "secret-token").unwrap();

        assert!(
            cipher_a.decrypt_for_user(11, &enc).is_err(),
            "AAD binding must prevent cross-user ciphertext swapping"
        );
        assert!(
            cipher_b.decrypt_for_user(10, &enc).is_err(),
            "wrong key must fail decryption"
        );
        assert!(
            cipher_a.decrypt_for_user(10, "plaintext-token").is_err(),
            "plaintext values must be rejected"
        );
    }

    #[test]
    fn rejects_invalid_key_lengths() {
        assert!(TokenCipher::from_key_str("").is_err());
        assert!(TokenCipher::from_key_str("short").is_err());
        assert!(TokenCipher::from_key_str("0123456789abcdef").is_err());
    }
}
