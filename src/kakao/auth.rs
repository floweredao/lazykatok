//! Resolve the KakaoTalk `user_id` and device `uuid` needed to derive the
//! SQLCipher key, then verify it by actually opening a database.
//!
//! Explicit overrides win. Automatic identities are verified in this order,
//! preferring the most recently modified openable database (including WAL):
//!   a. explicit override (config / `KATOK_KAKAO_USER_ID`)
//!   b. katok cache `<data_dir>/kakao/auth.json` ({user_id, uuid}, 0600)
//!   c. k-skill cache bootstrap (`~/.cache/k-skill/kakaotalk-mac-auth.json`,
//!      read ONLY `user_id`)
//!   d. preference plist candidates, newest plist first
//!   e. rayon SHA-512 recovery of each active preference hash, newest first
//!
//! Privacy: only `{user_id, uuid}` is ever persisted by katok, never the key.

use std::path::{Path, PathBuf};
use std::process::Command;

use super::derive;
use super::reader::probe_database;
use crate::{Error, Result};

pub use super::store::{
    app_support_dir, container_dir, discover_database_files, discover_databases,
};

mod identity;
mod resolve;
#[cfg(test)]
mod resolve_tests;
#[cfg(test)]
mod tests;

use identity::*;

const DEFAULT_MAX_USER_ID: i64 = 1_000_000_000;

/// A cheap, side-effect-free snapshot of KakaoTalk readiness. Contains ONLY
/// booleans/counts derived from filesystem and plist presence checks — it never
/// decrypts a database, runs the SHA-512 recovery, or reads message content.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ProbeStatus {
    /// The KakaoTalk macOS preference plist(s) or a store root exist.
    pub app_installed: bool,
    /// At least one encrypted-DB store root is present.
    pub container_present: bool,
    /// Count of `^[0-9a-f]{78}(?:\.db)?$` database files across the store
    /// roots, counting a filename present in both roots once.
    pub db_file_count: usize,
    /// The katok `{user_id, uuid}` auth cache file exists.
    pub auth_cached: bool,
}

/// Side-effect-free probe of KakaoTalk readiness for the doctor command. Only
/// inspects filesystem/plist presence and whether the katok auth cache exists;
/// it never decrypts, never runs SHA-512 recovery, and never reads messages.
pub fn probe_status(home: &Path, data_dir: &Path) -> ProbeStatus {
    let container_present = super::store::store_dirs(home)
        .iter()
        .any(|dir| dir.is_dir());
    let db_file_count = discover_databases(home).len();
    let app_installed = !preference_paths(home).is_empty() || container_present;
    let auth_cached = katok_cache_path(data_dir).is_file();
    ProbeStatus {
        app_installed,
        container_present,
        db_file_count,
        auth_cached,
    }
}

/// A verified `(user_id, uuid)` plus the discovered openable database files.
#[derive(Debug, Clone)]
pub struct ResolvedAuth {
    pub user_id: i64,
    pub uuid: String,
    pub source: &'static str,
    pub database_files: Vec<PathBuf>,
}

/// Inputs for auth resolution. The home directory and explicit overrides are
/// injectable so unit tests need no system state.
#[derive(Debug, Clone)]
pub struct AuthOptions {
    pub home: PathBuf,
    pub data_dir: PathBuf,
    pub user_id_override: Option<i64>,
    pub uuid_override: Option<String>,
    pub max_user_id: i64,
}

impl AuthOptions {
    pub fn new(home: PathBuf, data_dir: PathBuf) -> Self {
        let user_id_override = std::env::var("KATOK_KAKAO_USER_ID")
            .ok()
            .and_then(|raw| raw.trim().parse::<i64>().ok())
            .filter(|id| *id > 0);
        Self {
            home,
            data_dir,
            user_id_override,
            uuid_override: None,
            max_user_id: DEFAULT_MAX_USER_ID,
        }
    }
}

fn katok_cache_path(data_dir: &Path) -> PathBuf {
    data_dir.join("kakao").join("auth.json")
}

fn k_skill_cache_path(home: &Path) -> PathBuf {
    home.join(".cache")
        .join("k-skill")
        .join("kakaotalk-mac-auth.json")
}

fn preference_paths(home: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let pref_dir = home
        .join("Library")
        .join("Containers")
        .join("com.kakao.KakaoTalkMac")
        .join("Data")
        .join("Library")
        .join("Preferences");
    if let Ok(entries) = std::fs::read_dir(&pref_dir) {
        let mut matched: Vec<PathBuf> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| {
                        name.starts_with("com.kakao.KakaoTalkMac") && name.ends_with(".plist")
                    })
            })
            .collect();
        matched.sort();
        paths.extend(matched);
    }
    let global = home
        .join("Library")
        .join("Preferences")
        .join("com.kakao.KakaoTalkMac.plist");
    if global.exists() && !paths.contains(&global) {
        paths.push(global);
    }
    paths.sort_by_cached_key(|path| {
        std::cmp::Reverse(
            std::fs::metadata(path)
                .and_then(|meta| meta.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
        )
    });
    paths
}

fn platform_uuid() -> Result<String> {
    let output = Command::new("/usr/sbin/ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
        .map_err(|err| Error::Kakao(format!("failed to run ioreg: {err}")))?;
    if !output.status.success() {
        return Err(Error::Kakao("ioreg exited non-zero".to_string()));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    extract_platform_uuid(&stdout)
        .ok_or_else(|| Error::Kakao("could not read IOPlatformUUID from ioreg".to_string()))
}

fn extract_platform_uuid(ioreg_output: &str) -> Option<String> {
    let needle = "\"IOPlatformUUID\" = \"";
    let start = ioreg_output.find(needle)? + needle.len();
    let rest = &ioreg_output[start..];
    let end = rest.find('"')?;
    let candidate = &rest[..end];
    is_platform_uuid(candidate).then(|| candidate.to_string())
}

/// 36 ASCII hex digits and dashes, the shape of an IOPlatformUUID. Key
/// derivation slices the uuid by byte offset, so anything else must not reach it.
fn is_platform_uuid(candidate: &str) -> bool {
    candidate.len() == 36
        && candidate
            .bytes()
            .all(|b| b.is_ascii_hexdigit() || b == b'-')
}

/// Verify a `(user_id, uuid)` by deriving its key and opening any DB. Returns
/// the openable DB list when the key works against at least one file.
fn verify(user_id: i64, uuid: &str, database_files: &[PathBuf]) -> Option<Vec<PathBuf>> {
    let key = derive::secure_key(user_id, uuid);
    let derived_name = derive::database_name(user_id, uuid);
    let derived_name_db = format!("{derived_name}.db");
    // Probe the derived-name DB first, then any other candidate. Mirror the
    // reference `prioritized_database_paths`, which treats both `<derived>` and
    // `<derived>.db` as the prioritized derived match.
    let is_derived = |path: &PathBuf| -> bool {
        path.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == derived_name || n == derived_name_db)
    };
    let mut ordered: Vec<&PathBuf> = database_files
        .iter()
        .filter(|path| is_derived(path))
        .collect();
    ordered.extend(database_files.iter().filter(|path| !is_derived(path)));

    let mut openable: Vec<PathBuf> = Vec::new();
    for path in &ordered {
        if probe_database(path, &key).unwrap_or(false) {
            openable.push((*path).clone());
        }
    }
    if openable.is_empty() {
        None
    } else {
        Some(openable)
    }
}

/// Resolve and verify auth following the documented precedence chain.
pub fn resolve_auth(options: &AuthOptions) -> Result<ResolvedAuth> {
    let uuid = match &options.uuid_override {
        Some(uuid) => uuid.clone(),
        None => platform_uuid()?,
    };
    resolve::resolve_with_uuid(options, uuid)
}
