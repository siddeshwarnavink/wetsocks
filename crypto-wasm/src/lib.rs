use rand_core::OsRng;
use secp256k1::{PublicKey, Secp256k1, SecretKey};
use serde::Serialize;
use serde_json;
use wasm_bindgen::prelude::*;

#[derive(Serialize)]
pub struct KeyPair {
    #[serde(rename = "privateKey")]
    pub private_key: String,
    #[serde(rename = "publicKey")]
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
