//! Verified account selection.

use super::*;
use std::time::SystemTime;

/// Resolve and verify auth following the documented precedence chain.
pub(super) fn resolve_with_uuid(options: &AuthOptions, uuid: String) -> Result<ResolvedAuth> {
    let database_files = discover_databases(&options.home);
    if database_files.is_empty() {
        return Err(Error::Kakao(
            "no KakaoTalk database files found in any store directory".to_string(),
        ));
    }

    let katok_cache = katok_cache_path(&options.data_dir);

    // a. explicit override
    if let Some(user_id) = options.user_id_override {
        if let Some(openable) = verify(user_id, &uuid, &database_files) {
            persist_katok_cache(&katok_cache, user_id, &uuid);
            return Ok(ResolvedAuth {
                user_id,
                uuid,
                source: "override",
                database_files: openable,
            });
        }
    }

    // Keep source precedence for equally active stores, not for stale accounts.
    let mut candidates = Vec::new();
    if options.uuid_override.is_none() {
        if let Some((cached_id, cached_uuid)) = read_katok_cache(&katok_cache) {
            candidates.push((cached_id, cached_uuid, "katok-cache"));
        }
    }
    if let Some(user_id) = read_user_id_from_json(&k_skill_cache_path(&options.home)) {
        candidates.push((user_id, uuid.clone(), "k-skill-cache"));
    }

    let mut active_hashes = Vec::new();
    for path in preference_paths(&options.home) {
        if let Some((ids, hash)) = read_plist(&path) {
            for user_id in ids {
                if !candidates
                    .iter()
                    .any(|(id, device, _)| *id == user_id && device == &uuid)
                {
                    candidates.push((user_id, uuid.clone(), "plist"));
                }
            }
            if let Some(hash) = hash {
                if !active_hashes.contains(&hash) {
                    active_hashes.push(hash);
                }
            }
        }
    }

    let newest_database = database_files
        .iter()
        .map(|path| super::super::store::database_modified(path))
        .max()
        .unwrap_or(SystemTime::UNIX_EPOCH);
    let mut best: Option<(SystemTime, ResolvedAuth)> = None;
    let mut consider = |user_id, device: &str, source| {
        let Some(openable) = verify(user_id, device, &database_files) else {
            return false;
        };
        let modified = openable
            .iter()
            .map(|path| super::super::store::database_modified(path))
            .max()
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if best
            .as_ref()
            .is_none_or(|(best_modified, _)| modified > *best_modified)
        {
            best = Some((
                modified,
                ResolvedAuth {
                    user_id,
                    uuid: device.to_string(),
                    source,
                    database_files: openable,
                },
            ));
        }
        modified == newest_database
    };
    let mut found_newest = false;
    for (user_id, device, source) in &candidates {
        if consider(*user_id, device, *source) {
            found_newest = true;
            break;
        }
    }

    // Recover each preference hash newest-first, even after a stale cache works.
    if !found_newest && !active_hashes.is_empty() {
        eprintln!("lazykatok: recovering KakaoTalk user id (one-time SHA-512 scan)...");
        for hash in active_hashes {
            if let Some(user_id) = recover_user_id_from_sha512(&hash, options.max_user_id) {
                if !candidates
                    .iter()
                    .any(|(id, device, _)| *id == user_id && device == &uuid)
                    && consider(user_id, &uuid, "sha512-recovery")
                {
                    break;
                }
            }
        }
    }

    if let Some((_, resolved)) = best {
        if resolved.source != "katok-cache" {
            persist_katok_cache(&katok_cache, resolved.user_id, &resolved.uuid);
        }
        return Ok(resolved);
    }

    Err(Error::Kakao(
        "could not resolve a working KakaoTalk user id; set KATOK_KAKAO_USER_ID".to_string(),
    ))
}
