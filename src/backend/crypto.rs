use aes_gcm::{Aes256Gcm, KeyInit, Nonce, aead::Aead};
use rand::RngCore;

pub fn encrypt(master_key: &[u8; 32], plaintext: &[u8]) -> Result<Vec<u8>, crate::Error> {
    let cipher = Aes256Gcm::new(master_key.into());
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext)
        .map_err(|e| crate::Error::Custom(format!("encryption failed: {e:?}")))?;
    Ok([nonce_bytes.as_slice(), &ciphertext].concat())
}

pub fn decrypt(master_key: &[u8; 32], stored: &[u8]) -> Result<Vec<u8>, crate::Error> {
    if stored.len() < 12 {
        return Err(crate::Error::Custom(
            "encrypted payload is too short".to_string(),
        ));
    }

    let cipher = Aes256Gcm::new(master_key.into());
    let (nonce_bytes, ciphertext) = stored.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| crate::Error::Custom(format!("decryption failed: {e:?}")))
}
