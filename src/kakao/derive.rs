//! Native port of KakaoTalk's macOS key derivation.
//!
//! These functions are a 1:1 port of the proven reference algorithm
//! (`secure_key`, `database_name`, `hashed_device_uuid` from `kakaotalk_mac.py`,
//! which itself mirrors kakaocli's Swift `KeyDerivation`). String slicing is by
//! character, as in the reference, so a uuid that is short or not ASCII (a
//! corrupt auth cache) derives a key that simply fails to open the store rather
//! than panicking. For the normal hex-and-dash IOPlatformUUID this is the same
//! as slicing by byte.

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use hmac::Hmac;
use sha1::Sha1;
use sha2::{Digest, Sha256};

const PBKDF2_ROUNDS: u32 = 100_000;
const PBKDF2_DKLEN: usize = 128;

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn pbkdf2_sha256(password: &[u8], salt: &[u8]) -> [u8; PBKDF2_DKLEN] {
    let mut out = [0u8; PBKDF2_DKLEN];
    pbkdf2::pbkdf2::<Hmac<Sha256>>(password, salt, PBKDF2_ROUNDS, &mut out)
        .expect("pbkdf2 output length is valid");
    out
}

/// `base64_standard( sha1(uuid)[20] ++ sha256(uuid)[32] )`.
pub fn hashed_device_uuid(uuid: &str) -> String {
    let uuid_bytes = uuid.as_bytes();
    let mut combined = Vec::with_capacity(20 + 32);
    combined.extend_from_slice(&Sha1::digest(uuid_bytes));
    combined.extend_from_slice(&Sha256::digest(uuid_bytes));
    BASE64_STANDARD.encode(combined)
}

fn reverse_ascii(input: &str) -> String {
    input.chars().rev().collect()
}

/// `input[start..end]` by character, clamped like a Python slice.
fn char_slice(input: &str, start: usize, end: usize) -> String {
    input
        .chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

type LastDerived = std::sync::Mutex<Option<(i64, String, String)>>;

/// Each derivation is 100k PBKDF2 rounds, and a watch poll asks for the same
/// `(user_id, uuid)` several times, so remember the most recent result.
fn remember_last(
    last: &LastDerived,
    user_id: i64,
    uuid: &str,
    derive: impl FnOnce() -> String,
) -> String {
    let mut last = last.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((id, cached_uuid, value)) = last.as_ref() {
        if *id == user_id && cached_uuid == uuid {
            return value.clone();
        }
    }
    let value = derive();
    *last = Some((user_id, uuid.to_string(), value.clone()));
    value
}

/// The 256-lowercase-hex SQLCipher passphrase for `(user_id, uuid)`.
pub fn secure_key(user_id: i64, uuid: &str) -> String {
    static LAST: LastDerived = std::sync::Mutex::new(None);
    remember_last(&LAST, user_id, uuid, || derive_secure_key(user_id, uuid))
}

fn derive_secure_key(user_id: i64, uuid: &str) -> String {
    let hashed = hashed_device_uuid(uuid);
    let parts = [
        "A",
        &hashed,
        "|",
        "F",
        &char_slice(uuid, 0, 5),
        "H",
        &user_id.to_string(),
        "|",
        &char_slice(uuid, 7, usize::MAX),
    ];
    let joined = parts.join("F");
    let hawawa = reverse_ascii(&joined);
    let uuid_chars = uuid.chars().count();
    let salt_start = (uuid_chars as f64 * 0.3) as usize;
    let salt = char_slice(uuid, salt_start, usize::MAX);
    let derived = pbkdf2_sha256(hawawa.as_bytes(), salt.as_bytes());
    hex_lower(&derived)
}

/// The 78-hex DB filename for `(user_id, uuid)`.
pub fn database_name(user_id: i64, uuid: &str) -> String {
    static LAST: LastDerived = std::sync::Mutex::new(None);
    remember_last(&LAST, user_id, uuid, || derive_database_name(user_id, uuid))
}

fn derive_database_name(user_id: i64, uuid: &str) -> String {
    let reversed_uuid = reverse_ascii(uuid);
    let parts = [
        ".",
        "F",
        &user_id.to_string(),
        "A",
        "F",
        &reversed_uuid,
        ".",
        "|",
    ];
    let hawawa = parts.join(".");
    let salt = reverse_ascii(&hashed_device_uuid(uuid));
    let derived = pbkdf2_sha256(hawawa.as_bytes(), salt.as_bytes());
    let hex = hex_lower(&derived);
    hex[28..28 + 78].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYNTHETIC_UUID: &str = "00000000-1111-2222-3333-444444444444";
    const SYNTHETIC_USER_ID: i64 = 1_000_000_001;

    #[test]
    fn database_name_matches_independent_synthetic_vector() {
        // Expected bytes were calculated independently with Python's
        // hashlib.pbkdf2_hmac from these explicitly synthetic inputs.
        assert_eq!(
            database_name(SYNTHETIC_USER_ID, SYNTHETIC_UUID),
            "de345a8eb68ff0db3c1f8b94817936a00471d335162afc05cdfc758f638a33d427ea7742d4d420"
        );
    }

    #[test]
    fn secure_key_is_256_lowercase_hex() {
        let key = secure_key(SYNTHETIC_USER_ID, SYNTHETIC_UUID);
        assert_eq!(key.len(), 256);
        assert!(key
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
    }

    #[test]
    fn secure_key_matches_independent_synthetic_vector() {
        // Computed with Python's hashlib.pbkdf2_hmac from synthetic inputs.
        assert_eq!(
            derive_secure_key(SYNTHETIC_USER_ID, SYNTHETIC_UUID),
            concat!(
                "30231c3546eada7fee9584912ce5fb779cf9699b3267c6910f390039ad5f0ba9",
                "a144fc24a28375ee22c12396fc713f0b0b85a6b337d203ed64177eef9dbfbc7d",
                "379920e8ba47ee805a1a2f5bd8e11dcd3680fa1375972596981725646f06f618",
                "83a4bd816b3faaa062d0222707bbd4bfb6631f52ef510dca97308f116690e7a0",
            )
        );
    }

    #[test]
    fn secure_key_slices_a_short_or_non_ascii_uuid_by_character() {
        // A corrupt auth cache can hand over a uuid that is not 36 ASCII bytes.
        // The reference slices by character and never fails; these vectors come
        // from that algorithm in Python (`hashlib.pbkdf2_hmac`) on synthetic input.
        assert_eq!(
            derive_secure_key(SYNTHETIC_USER_ID, "abc"),
            concat!(
                "018e64d9d5a38d4e7971dc7c4305326a692c9dd61761a711e0bb056bc0832440",
                "b967c21216b8e2fad7f1a44536acd7a7d150f44af525000066192d0a4d941881",
                "a2d556aaa1fbb5122f6d75406fbe2d14a23f6acb454fc0f2b6342d0f267963b8",
                "d760c7be679fc8d2d4e7d7326ac32b43b093fca264cc0c3c205e98c94fc1b650",
            )
        );
        assert_eq!(
            derive_secure_key(SYNTHETIC_USER_ID, "가나다라마바사아자차"),
            concat!(
                "a5c5c0097519e0ca028ef36294b78b7ccdc9f892548f65c5b2b4dcece46cda2e",
                "146a770747065143818d5efba240b8c1a10831c5c95b7535d1daec138e64e593",
                "eb69b13538a35470eb8305296e266e8bdc93bf91fb6ceb122533b97bf4fba884",
                "2314b4611911f17987814d73f5fc190c3d3841e6f81b5816462c503be03e1576",
            )
        );
    }

    #[test]
    fn remember_last_derives_once_per_identity() {
        let last = LastDerived::new(None);
        let first = remember_last(&last, 1, SYNTHETIC_UUID, || "one".to_string());
        let again = remember_last(&last, 1, SYNTHETIC_UUID, || {
            panic!("the same identity must not be derived twice in a row")
        });
        let other = remember_last(&last, 2, SYNTHETIC_UUID, || "two".to_string());
        assert_eq!(
            (first.as_str(), again.as_str(), other.as_str()),
            ("one", "one", "two")
        );
    }

    #[test]
    fn hashed_device_uuid_is_base64_of_52_bytes() {
        let hashed = hashed_device_uuid(SYNTHETIC_UUID);
        let decoded = BASE64_STANDARD.decode(&hashed).expect("valid base64");
        assert_eq!(decoded.len(), 20 + 32);
    }
}
