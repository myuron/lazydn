use ssh_key::rand_core::{OsRng, RngCore};

/// 32 lowercase hex characters, the shape the server accepts.
pub fn generate() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
