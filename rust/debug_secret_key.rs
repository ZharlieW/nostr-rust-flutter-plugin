use nostr::key::Keys;

fn main() {
    let keys = Keys::generate();
    let secret_key = keys.secret_key();
    println!("Type: {}", std::any::type_name_of_val(secret_key));
    println!("Secret key: {}", secret_key.to_secret_hex());
}
