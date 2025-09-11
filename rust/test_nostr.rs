use rust_lib_nostr_rust::api::nostr::*;

fn main() {
    println!("Testing Nostr Rust functions...");
    
    // Test generate_keys
    match generate_keys() {
        Ok(keys) => {
            println!("✅ Generated keys successfully:");
            println!("  Public key: {}", keys.public_key);
            println!("  Private key: {}", keys.private_key);
            
            // Test NIP-04 encryption/decryption
            let plaintext = "Hello, Nostr!";
            match nip04_encrypt(plaintext.to_string(), keys.public_key.clone(), keys.private_key.clone()) {
                Ok(encrypted) => {
                    println!("✅ NIP-04 encryption successful: {}", encrypted);
                    
                    match nip04_decrypt(encrypted, keys.public_key.clone(), keys.private_key.clone()) {
                        Ok(decrypted) => {
                            println!("✅ NIP-04 decryption successful: {}", decrypted);
                            if decrypted == plaintext {
                                println!("✅ NIP-04 round-trip test passed!");
                            } else {
                                println!("❌ NIP-04 round-trip test failed!");
                            }
                        }
                        Err(e) => println!("❌ NIP-04 decryption failed: {}", e),
                    }
                }
                Err(e) => println!("❌ NIP-04 encryption failed: {}", e),
            }
            
            // Test NIP-44 encryption/decryption
            match nip44_encrypt(plaintext.to_string(), keys.public_key.clone(), keys.private_key.clone()) {
                Ok(encrypted) => {
                    println!("✅ NIP-44 encryption successful: {}", encrypted);
                    
                    match nip44_decrypt(encrypted, keys.public_key.clone(), keys.private_key.clone()) {
                        Ok(decrypted) => {
                            println!("✅ NIP-44 decryption successful: {}", decrypted);
                            if decrypted == plaintext {
                                println!("✅ NIP-44 round-trip test passed!");
                            } else {
                                println!("❌ NIP-44 round-trip test failed!");
                            }
                        }
                        Err(e) => println!("❌ NIP-44 decryption failed: {}", e),
                    }
                }
                Err(e) => println!("❌ NIP-44 encryption failed: {}", e),
            }
        }
        Err(e) => println!("❌ Failed to generate keys: {}", e),
    }
    
    println!("Test completed!");
}
