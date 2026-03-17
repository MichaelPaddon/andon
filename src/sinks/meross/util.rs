use std::time::{SystemTime, UNIX_EPOCH};

use md5::{Digest, Md5};

pub fn md5_hex(s: &str) -> String {
    let mut h = Md5::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

pub fn random_hex(n: usize) -> String {
    let bytes = (n + 1) / 2;
    (0..bytes)
        .map(|_| format!("{:02x}", rand::random::<u8>()))
        .collect::<String>()
        .chars()
        .take(n)
        .collect()
}

pub fn random_alnum_upper(n: usize) -> String {
    use rand::distributions::Alphanumeric;
    use rand::Rng;
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(n)
        .map(|b| (b as char).to_ascii_uppercase())
        .collect()
}

pub fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
