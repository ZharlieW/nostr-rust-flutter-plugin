use nostr::event::{EventBuilder, EventId, Kind, Tag, UnsignedEvent};
use nostr::key::{Keys, PublicKey, SecretKey};
use nostr::nips::nip04;
use nostr::nips::nip44;
use nostr::nips::nip49::{EncryptedSecretKey, KeySecurity};
use nostr::nips::nip19::{FromBech32, ToBech32};
use nostr::secp256k1::schnorr::Signature;
use nostr::types::time::Timestamp;
use nostr::util::JsonUtil;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Serialize, Deserialize)]
pub struct NostrEvent {
    pub id: String,
    pub pubkey: String,
    pub created_at: u64,
    pub kind: u64,
    pub tags: Vec<Vec<String>>,
    pub content: String,
    pub sig: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NostrKeys {
    pub public_key: String,
    pub private_key: String,
}

pub async fn generate_keys() -> Result<NostrKeys, String> {
    let keys = Keys::generate();
    Ok(NostrKeys {
        public_key: keys.public_key().to_hex(),
        private_key: keys.secret_key().to_secret_hex(),
    })
}

pub async fn get_public_key_from_private(private_key: String) -> Result<String, String> {
    let private_key =
        SecretKey::from_str(&private_key).map_err(|e| format!("Invalid private key: {}", e))?;

    let keys = Keys::new(private_key);
    Ok(keys.public_key().to_hex())
}

pub async fn nip04_encrypt(
    plaintext: String,
    public_key: String,
    private_key: String,
) -> Result<String, String> {
    let public_key =
        PublicKey::from_str(&public_key).map_err(|e| format!("Invalid public key: {}", e))?;
    let private_key =
        SecretKey::from_str(&private_key).map_err(|e| format!("Invalid private key: {}", e))?;

    let keys = Keys::new(private_key);
    let secret_key = keys.secret_key();
    let encrypted = nip04::encrypt(secret_key, &public_key, plaintext)
        .map_err(|e| format!("Encryption failed: {}", e))?;

    Ok(encrypted)
}

pub async fn nip04_decrypt(
    ciphertext: String,
    public_key: String,
    private_key: String,
) -> Result<String, String> {
    let public_key =
        PublicKey::from_str(&public_key).map_err(|e| format!("Invalid public key: {}", e))?;
    let private_key =
        SecretKey::from_str(&private_key).map_err(|e| format!("Invalid private key: {}", e))?;

    let keys = Keys::new(private_key);
    let secret_key = keys.secret_key();
    let decrypted = nip04::decrypt(secret_key, &public_key, ciphertext)
        .map_err(|e| format!("Decryption failed: {}", e))?;

    Ok(decrypted)
}

pub async fn nip44_encrypt(
    plaintext: String,
    public_key: String,
    private_key: String,
) -> Result<String, String> {
    let public_key =
        PublicKey::from_str(&public_key).map_err(|e| format!("Invalid public key: {}", e))?;
    let private_key =
        SecretKey::from_str(&private_key).map_err(|e| format!("Invalid private key: {}", e))?;

    let keys = Keys::new(private_key);
    let secret_key = keys.secret_key();
    let encrypted = nip44::encrypt(secret_key, &public_key, plaintext, nip44::Version::V2)
        .map_err(|e| format!("NIP-44 encryption failed: {}", e))?;

    Ok(encrypted)
}

pub async fn nip44_decrypt(
    ciphertext: String,
    public_key: String,
    private_key: String,
) -> Result<String, String> {
    let public_key =
        PublicKey::from_str(&public_key).map_err(|e| format!("Invalid public key: {}", e))?;
    let private_key =
        SecretKey::from_str(&private_key).map_err(|e| format!("Invalid private key: {}", e))?;

    let keys = Keys::new(private_key);
    let secret_key = keys.secret_key();
    let decrypted = nip44::decrypt(secret_key, &public_key, ciphertext)
        .map_err(|e| format!("NIP-44 decryption failed: {}", e))?;

    Ok(decrypted)
}

pub async fn sign_event(event_json: String, private_key: String) -> Result<String, String> {
    let private_key =
        SecretKey::from_str(&private_key).map_err(|e| format!("Invalid private key: {}", e))?;

    let keys = Keys::new(private_key);

    // Parse the unsigned event directly from JSON using nostr's built-in method
    // This ensures all fields (including tags) are preserved correctly
    let unsigned_event = UnsignedEvent::from_json(&event_json)
        .map_err(|e| format!("Failed to parse event JSON: {}", e))?;

    // Sign the unsigned event
    // This will compute the event ID, sign it, and create a complete Event
    let event = unsigned_event
        .sign_with_keys(&keys)
        .map_err(|e| format!("Failed to sign event: {}", e))?;

    // Convert back to JSON string
    let signed_event_json = event
        .try_as_json()
        .map_err(|e| format!("Failed to serialize signed event: {}", e))?;

    Ok(signed_event_json)
}

pub async fn verify_event(event: NostrEvent) -> Result<bool, String> {
    let event_id = EventId::from_str(&event.id).map_err(|e| format!("Invalid event ID: {}", e))?;
    let pubkey =
        PublicKey::from_str(&event.pubkey).map_err(|e| format!("Invalid public key: {}", e))?;
    let sig = Signature::from_str(&event.sig).map_err(|e| format!("Invalid signature: {}", e))?;

    // Convert tags back to nostr format
    let tags: Vec<Tag> = event
        .tags
        .into_iter()
        .map(|tag_vec| {
            let tag_strings: Vec<String> = tag_vec.into_iter().map(|s| s.to_string()).collect();
            Tag::parse(&tag_strings)
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Invalid tags: {}", e))?;

    // Create the event for verification using EventBuilder
    let nostr_event = EventBuilder::new(Kind::from(event.kind as u16), event.content)
        .tags(tags)
        .custom_created_at(Timestamp::from(event.created_at))
        .sign_with_keys(&Keys::new(SecretKey::from_str(&event.pubkey).unwrap()))
        .unwrap();

    // Verify the signature
    Ok(nostr_event.verify().is_ok())
}

/// Encrypt private key using NIP-49 (ncryptsec1)
/// 
/// # Arguments
/// * `private_key` - Private key in hex format (64 characters)
/// * `password` - Password for encryption
/// * `log_n` - Scrypt log2(N) parameter (12-22, default 16)
/// * `key_security` - Key security level: 0=Weak, 1=Medium, 2=Unknown (default)
pub async fn nip49_encrypt(
    private_key: String,
    password: String,
    log_n: Option<u8>,
    key_security: Option<u8>,
) -> Result<String, String> {
    let secret_key =
        SecretKey::from_str(&private_key).map_err(|e| format!("Invalid private key: {}", e))?;

    let log_n = log_n.unwrap_or(16);
    if log_n < 12 || log_n > 22 {
        return Err("log_n must be between 12 and 22".to_string());
    }

    let key_security = match key_security.unwrap_or(2) {
        0 => KeySecurity::Weak,
        1 => KeySecurity::Medium,
        2 => KeySecurity::Unknown,
        v => return Err(format!("Invalid key_security: {v}, must be 0, 1, or 2")),
    };

    let encrypted = EncryptedSecretKey::new(&secret_key, &password, log_n, key_security)
        .map_err(|e| format!("NIP-49 encryption failed: {}", e))?;

    let ncryptsec = encrypted
        .to_bech32()
        .map_err(|e| format!("Failed to encode to bech32: {}", e))?;

    Ok(ncryptsec)
}

/// Decrypt private key from NIP-49 (ncryptsec1) format
/// 
/// # Arguments
/// * `ncryptsec` - Encrypted private key in ncryptsec1 format
/// * `password` - Password for decryption
pub async fn nip49_decrypt(ncryptsec: String, password: String) -> Result<String, String> {
    let encrypted = EncryptedSecretKey::from_bech32(&ncryptsec)
        .map_err(|e| format!("Invalid ncryptsec format: {}", e))?;

    let secret_key = encrypted
        .decrypt(&password)
        .map_err(|e| format!("NIP-49 decryption failed: {}", e))?;

    Ok(secret_key.to_secret_hex())
}

pub async fn greet(name: String) -> String {
    format!("Hello, {name}!")
}

#[flutter_rust_bridge::frb(init)]
pub fn init_app() {
    // Default utilities - feel free to customize
    flutter_rust_bridge::setup_default_user_utils();
}
