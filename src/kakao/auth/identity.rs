//! Preference and cached identity discovery.

use super::*;
use rayon::prelude::*;
use sha2::{Digest, Sha512};

pub(super) const EMPTY_ACCOUNT_HASH: &str =
    "31bca02094eb78126a517b206a88c73cfa9ec6f704c7030d18212cace820f025\
f00bf0ea68dbf3f3a5436ca63b53bf7bf80ad8d5de7d8359d0b7fed9dbc3ab99";
const DIRECT_USER_ID_KEYS: [&str; 4] = ["userId", "user_id", "KAKAO_USER_ID", "userID"];
const DESIGNATED_PREFIX: &str = "DESIGNATEDFRIENDSREVISION:";

/// True for an ASCII lowercase-hex byte (`0-9` or `a-f`), matching the
/// reference `[0-9a-f]` character class. Uppercase is intentionally rejected.
fn is_lower_hex(b: u8) -> bool {
    b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
}

/// Best-effort parse of a preference plist into `(candidate_user_ids,
/// active_account_hash)` via `plutil -convert xml1`.
pub(super) fn read_plist(path: &Path) -> Option<(Vec<i64>, Option<String>)> {
    let output = Command::new("/usr/bin/plutil")
        .args(["-convert", "xml1", "-o", "-"])
        .arg(path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let xml = String::from_utf8_lossy(&output.stdout);
    Some(scan_plist_xml(&xml))
}

/// Tolerant scan of plist XML. Splitting on `<` yields fragments of the form
/// `tag>body`; for `<key>NAME</key>` the key name is the body of the `key`
/// fragment, and the value element is the next non-`/key` fragment. Only the
/// structure is read, never message bodies.
pub(super) fn scan_plist_xml(xml: &str) -> (Vec<i64>, Option<String>) {
    let mut candidates: Vec<i64> = Vec::new();
    let mut active_hash: Option<String> = None;
    let mut in_alert_array = false;
    let mut pending_key: Option<String> = None;

    for raw in xml.split('<') {
        let Some(end) = raw.find('>') else {
            continue;
        };
        // Empty elements are self-closing (`<true/>`); name them like their open form.
        let tag = raw[..end].trim_end_matches('/');
        let body = raw[end + 1..].trim();

        // <key>NAME</key>: the key name is the body of this fragment.
        if tag == "key" {
            pending_key = Some(body.to_string());
            in_alert_array = body == "AlertKakaoIDsList";
            continue;
        }
        if tag == "/key" {
            continue;
        }

        // Value element paired with the most recent key.
        if let Some(key) = pending_key.clone() {
            if DIRECT_USER_ID_KEYS.contains(&key.as_str()) {
                if let Some(value) = parse_scalar_value(tag, body) {
                    if value > 0 {
                        candidates.push(value);
                    }
                }
                pending_key = None;
                continue;
            }
            if let Some(hash_hex) = key.strip_prefix(DESIGNATED_PREFIX) {
                if active_hash.is_none() && is_active_account_hash(hash_hex, tag, body) {
                    active_hash = Some(hash_hex.to_string());
                }
                pending_key = None;
                continue;
            }
        }

        // AlertKakaoIDsList array members.
        if in_alert_array {
            if tag == "/array" {
                in_alert_array = false;
                pending_key = None;
            } else if let Some(value) = parse_scalar_value(tag, body) {
                if value > 0 {
                    candidates.push(value);
                }
            }
        }
    }

    candidates.sort_unstable();
    candidates.dedup();
    (candidates, active_hash)
}

pub(super) fn parse_scalar_value(tag: &str, body: &str) -> Option<i64> {
    match tag {
        "integer" | "real" | "string" => body.trim().parse::<i64>().ok(),
        _ => None,
    }
}

pub(super) fn is_active_account_hash(hash_hex: &str, value_tag: &str, value_body: &str) -> bool {
    if hash_hex == EMPTY_ACCOUNT_HASH {
        return false;
    }
    // Match the reference regex `[0-9a-f]{128}`: lowercase hex only. SHA-512
    // hexdigests are always lowercase, so an uppercase key is rejected here
    // rather than fed into a fruitless full 0..=1e9 scan.
    if hash_hex.len() != 128 || !hash_hex.bytes().all(is_lower_hex) {
        return false;
    }
    match value_tag {
        "integer" | "real" => value_body
            .trim()
            .parse::<i64>()
            .map(|v| v != 0)
            .unwrap_or(false),
        "true" => true,
        _ => false,
    }
}

/// Read ONLY `user_id` from a JSON cache file.
pub(super) fn read_user_id_from_json(path: &Path) -> Option<i64> {
    let raw = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    match value.get("user_id")? {
        serde_json::Value::Number(num) => num.as_i64().filter(|id| *id > 0),
        serde_json::Value::String(text) => text.trim().parse::<i64>().ok().filter(|id| *id > 0),
        _ => None,
    }
}

/// Read `{user_id, uuid}` from the katok cache (both required).
pub(super) fn read_katok_cache(path: &Path) -> Option<(i64, String)> {
    let raw = std::fs::read_to_string(path).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let user_id = match value.get("user_id")? {
        serde_json::Value::Number(num) => num.as_i64()?,
        serde_json::Value::String(text) => text.trim().parse::<i64>().ok()?,
        _ => return None,
    };
    let uuid = value.get("uuid")?.as_str()?.to_string();
    if user_id > 0 && is_platform_uuid(&uuid) {
        Some((user_id, uuid))
    } else {
        None
    }
}

/// Persist only `{user_id, uuid}` at 0600 for the next run.
pub(super) fn persist_katok_cache(path: &Path, user_id: i64, uuid: &str) {
    if let Some(parent) = path.parent() {
        if crate::paths::ensure_private_dir(parent).is_err() {
            return;
        }
    }
    let payload = serde_json::json!({ "user_id": user_id, "uuid": uuid });
    let Ok(serialized) = serde_json::to_string_pretty(&payload) else {
        return;
    };
    // Write to a temp file in the same dir, created 0600, then atomically rename
    // into place. This avoids both the umask window (the file is never group/
    // world-readable, even briefly) and a torn write of partial JSON.
    let Some(parent) = path.parent() else {
        return;
    };
    let tmp = parent.join(format!(".auth.json.tmp.{}", std::process::id()));
    if write_private_file(&tmp, serialized.as_bytes()).is_err() {
        let _ = std::fs::remove_file(&tmp);
        return;
    }
    if std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Create `path` with mode 0600 and write `bytes`, truncating any existing file.
pub(super) fn write_private_file(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Brute-force the `user_id` whose `sha512(user_id.to_string())` hex equals
/// `target_hash`, using rayon across `0..=max_user_id`.
pub(super) fn recover_user_id_from_sha512(target_hash: &str, max_user_id: i64) -> Option<i64> {
    // Lowercase hex only, matching the reference regex `[0-9a-f]{128}`. An
    // uppercase target can never match the lowercase digest comparison, so
    // reject it before launching the rayon scan.
    if target_hash.len() != 128 || !target_hash.bytes().all(is_lower_hex) {
        return None;
    }
    let target = target_hash.as_bytes();
    (0..=max_user_id).into_par_iter().find_any(|candidate| {
        let digest = Sha512::digest(candidate.to_string().as_bytes());
        // Compare hex without allocating per candidate.
        sha512_hex_eq(&digest, target)
    })
}

pub(super) fn sha512_hex_eq(digest: &[u8], target_hex: &[u8]) -> bool {
    if target_hex.len() != digest.len() * 2 {
        return false;
    }
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for (i, byte) in digest.iter().enumerate() {
        if target_hex[i * 2] != HEX[(byte >> 4) as usize]
            || target_hex[i * 2 + 1] != HEX[(byte & 0x0f) as usize]
        {
            return false;
        }
    }
    true
}
