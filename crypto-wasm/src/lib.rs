use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use rand_core::OsRng;
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use serde::Serialize;
use serde_json;
use sha2::{Digest, Sha256};
use wasm_bindgen::prelude::*;

#[derive(Serialize)]
pub struct KeyPair {
    pub private_key: String,
    pub public_key: String,
}

#[wasm_bindgen]
pub fn generate_keypair() -> Result<String, JsValue> {
    let secp = Secp256k1::new();
    let mut rng = OsRng;
    let secret_key = SecretKey::new(&mut rng);
    let public_key = PublicKey::from_secret_key(&secp, &secret_key);

    let keypair = KeyPair {
        private_key: hex::encode(secret_key.secret_bytes()),
        public_key: hex::encode(public_key.serialize_uncompressed()),
    };

    serde_json::to_string(&keypair).map_err(|e| {
        JsValue::from_str(&format!("Failed to serialize keypair: {}", e))
    })
}

#[wasm_bindgen]
pub fn encrypt_message(
    message: &str,
    recipient_public_key_hex: &str,
) -> Result<String, JsValue> {
    let secp = Secp256k1::new();
    let mut rng = OsRng;

    let recipient_pub_bytes =
        hex::decode(recipient_public_key_hex).map_err(|e| {
            JsValue::from_str(&format!("Invalid public key hex: {}", e))
        })?;
    let recipient_pub =
        PublicKey::from_slice(&recipient_pub_bytes).map_err(|e| {
            JsValue::from_str(&format!("Invalid public key: {}", e))
        })?;

    let ephemeral_sk = SecretKey::new(&mut rng);
    let ephemeral_pk = PublicKey::from_secret_key(&secp, &ephemeral_sk);

    let shared_point = recipient_pub.combine(&ephemeral_pk).map_err(|e| {
        JsValue::from_str(&format!("Failed to compute shared secret: {}", e))
    })?;
    let shared_secret = shared_point.serialize_uncompressed();

    let mut hasher = Sha256::new();
    hasher.update(&shared_secret);
    let encryption_key = hasher.finalize();

    let cipher = Aes256Gcm::new(encryption_key.as_slice().into());
    let nonce_bytes: [u8; 12] = rand::random();
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, message.as_bytes())
        .map_err(|e| JsValue::from_str(&format!("Encryption failed: {}", e)))?;

    let mut result = Vec::new();
    result.extend_from_slice(&ephemeral_pk.serialize_uncompressed());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);

    Ok(hex::encode(result))
}

#[wasm_bindgen]
pub fn decrypt_message(
    encrypted_hex: &str,
    private_key_hex: &str,
) -> Result<String, JsValue> {
    let secp = Secp256k1::new();

    let private_key_bytes = hex::decode(private_key_hex).map_err(|e| {
        JsValue::from_str(&format!("Invalid private key hex: {}", e))
    })?;
    let private_key =
        SecretKey::from_slice(&private_key_bytes).map_err(|e| {
            JsValue::from_str(&format!("Invalid private key: {}", e))
        })?;

    let encrypted_data = hex::decode(encrypted_hex).map_err(|e| {
        JsValue::from_str(&format!("Invalid encrypted data hex: {}", e))
    })?;

    if encrypted_data.len() < 65 + 12 {
        return Err(JsValue::from_str("Encrypted data too short"));
    }

    let ephemeral_pk_bytes = &encrypted_data[0..65];
    let nonce_bytes = &encrypted_data[65..77];
    let ciphertext = &encrypted_data[77..];

    let ephemeral_pk =
        PublicKey::from_slice(ephemeral_pk_bytes).map_err(|e| {
            JsValue::from_str(&format!("Invalid ephemeral public key: {}", e))
        })?;

    let own_pub = PublicKey::from_secret_key(&secp, &private_key);
    let shared_point = ephemeral_pk.combine(&own_pub).map_err(|e| {
        JsValue::from_str(&format!("Failed to compute shared secret: {}", e))
    })?;
    let shared_secret = shared_point.serialize_uncompressed();

    let mut hasher = Sha256::new();
    hasher.update(&shared_secret);
    let decryption_key = hasher.finalize();

    let cipher = Aes256Gcm::new(decryption_key.as_slice().into());
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| JsValue::from_str(&format!("Decryption failed: {}", e)))?;

    String::from_utf8(plaintext)
        .map_err(|e| JsValue::from_str(&format!("Invalid UTF-8: {}", e)))
}
