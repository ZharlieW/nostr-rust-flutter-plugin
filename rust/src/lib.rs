pub mod api;
mod frb_generated;

#[cfg(test)]
mod tests {
    use super::api::nostr::*;
    
    #[test]
    fn test_nostr_functions() {
        println!("Testing Nostr Rust functions...");
        
        // Test generate_keys
        let keys = generate_keys().unwrap();
        println!("✅ Generated keys successfully");
        
        // Test NIP-04 encryption/decryption
        let plaintext = "Hello, Nostr!";
        let encrypted = nip04_encrypt(plaintext.to_string(), keys.public_key.clone(), keys.private_key.clone()).unwrap();
        println!("✅ NIP-04 encryption successful");
        
        let decrypted = nip04_decrypt(encrypted, keys.public_key.clone(), keys.private_key.clone()).unwrap();
        assert_eq!(decrypted, plaintext);
        println!("✅ NIP-04 round-trip test passed!");
        
        // Test NIP-44 encryption/decryption
        let encrypted44 = nip44_encrypt(plaintext.to_string(), keys.public_key.clone(), keys.private_key.clone()).unwrap();
        println!("✅ NIP-44 encryption successful");
        
        let decrypted44 = nip44_decrypt(encrypted44, keys.public_key.clone(), keys.private_key.clone()).unwrap();
        assert_eq!(decrypted44, plaintext);
        println!("✅ NIP-44 round-trip test passed!");
        
        println!("All tests passed!");
    }
}
