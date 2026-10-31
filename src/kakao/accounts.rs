//! KakaoTalk accounts whose databases are on this Mac, and where each one's archive lives.
//!
//! KakaoTalk keeps a signed-out account's databases on disk, so a Mac that has used two accounts
//! holds both. Each database opens only with its own account's key, which makes "the key opens
//! a database" the test for an account, and keeps two accounts' archives apart: the account
//! pinned in `<data_dir>/kakao/auth.json` keeps `<data_dir>`, and any other account reads into
//! `<data_dir>/accounts/<user_id>`.
//!
//! Privacy: only user ids, counts, and timestamps are reported, never names or file names.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use sha2::{Digest, Sha512};

use super::auth::{
    k_skill_cache_path, katok_cache_path, persist_katok_cache, platform_uuid, preference_paths,
    read_katok_cache, read_plist, read_user_id_from_json, recover_user_id_from_sha512, verify,
    AuthOptions,
};
use super::store::{database_modified, discover_databases};
use crate::{Error, Result};

/// Directory under the base data dir that holds every non-pinned account's data dir.
pub const ACCOUNTS_DIR: &str = "accounts";

/// One account whose key opens at least one database on this Mac.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Account {
    pub user_id: i64,
    /// This account's databases were written most recently. KakaoTalk writes to the signed-in
    /// account's store as messages arrive, so this is the account in use; the preference files
    /// keep a record for every account that ever signed in and cannot tell which is current.
    pub latest: bool,
    /// Databases this account's key opens.
    pub databases: usize,
    /// Newest modification of those databases, including their WAL.
    pub last_modified: Option<DateTime<Utc>>,
    /// The katok data dir this account reads into.
    pub data_dir: PathBuf,
}

/// The data dir for `user_id` under `base`: `base` itself when its auth cache already pins that
/// account, or when no account is pinned there yet, else `base/accounts/<user_id>`.
pub fn account_data_dir(base: &Path, user_id: i64) -> PathBuf {
    match read_katok_cache(&katok_cache_path(base)) {
        Some((pinned, _)) if pinned != user_id => base.join(ACCOUNTS_DIR).join(user_id.to_string()),
        _ => base.to_path_buf(),
    }
}

/// User ids remembered by katok under `base`: the pinned one and every per-account dir.
fn cached_user_ids(base: &Path) -> Vec<i64> {
    let mut ids = Vec::new();
    if let Some((id, _)) = read_katok_cache(&katok_cache_path(base)) {
        ids.push(id);
    }
    if let Ok(entries) = std::fs::read_dir(base.join(ACCOUNTS_DIR)) {
        for entry in entries.flatten() {
            if let Some((id, _)) = read_katok_cache(&katok_cache_path(&entry.path())) {
                ids.push(id);
            }
        }
    }
    ids
}

fn sha512_hex(user_id: i64) -> String {
    Sha512::digest(user_id.to_string().as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Every account whose key opens a database, newest database first.
///
/// Candidates come from katok's own caches under `options.data_dir`, the k-skill cache, and
/// KakaoTalk's preferences, which keep one per-account file with a SHA-512 hash of its user id.
/// A hash no candidate matches is recovered by the SHA-512 scan once, and the id is cached in
/// that account's own data dir so the next listing skips the scan.
pub fn list_accounts(options: &AuthOptions) -> Result<Vec<Account>> {
    let database_files = discover_databases(&options.home);
    if database_files.is_empty() {
        return Err(Error::Kakao(
            "no KakaoTalk database files found in any store directory".to_string(),
        ));
    }
    let uuid = match &options.uuid_override {
        Some(uuid) => uuid.clone(),
        None => platform_uuid()?,
    };

    let mut candidates = cached_user_ids(&options.data_dir);
    candidates.extend(options.user_id_override);
    candidates.extend(read_user_id_from_json(&k_skill_cache_path(&options.home)));
    let mut active_hashes = Vec::new();
    for path in preference_paths(&options.home) {
        if let Some((ids, hash)) = read_plist(&path) {
            candidates.extend(ids);
            active_hashes.extend(hash);
        }
    }
    let mut recovered = Vec::new();
    for hash in &active_hashes {
        if !candidates.iter().any(|id| &sha512_hex(*id) == hash) {
            eprintln!("lazykatok: recovering a KakaoTalk user id (one-time SHA-512 scan)...");
            if let Some(id) = recover_user_id_from_sha512(hash, options.max_user_id) {
                candidates.push(id);
                recovered.push(id);
            }
        }
    }

    let mut accounts: BTreeMap<i64, Account> = BTreeMap::new();
    for user_id in candidates {
        if user_id <= 0 || accounts.contains_key(&user_id) {
            continue;
        }
        let Some(openable) = verify(user_id, &uuid, &database_files) else {
            continue;
        };
        let data_dir = account_data_dir(&options.data_dir, user_id);
        // Remember a scanned id, but never pin the base dir from a listing: the first account
        // actually read there takes it.
        if recovered.contains(&user_id) && data_dir != options.data_dir {
            persist_katok_cache(&katok_cache_path(&data_dir), user_id, &uuid);
        }
        let last_modified = openable
            .iter()
            .map(|path| database_modified(path))
            .max()
            .map(DateTime::<Utc>::from);
        accounts.insert(
            user_id,
            Account {
                user_id,
                latest: false,
                databases: openable.len(),
                last_modified,
                data_dir,
            },
        );
    }
    let mut accounts: Vec<Account> = accounts.into_values().collect();
    sort_accounts(&mut accounts);
    Ok(accounts)
}

/// Most recently written first, and mark that one as the latest.
fn sort_accounts(accounts: &mut [Account]) {
    accounts.sort_by(|a, b| {
        b.last_modified
            .cmp(&a.last_modified)
            .then(a.user_id.cmp(&b.user_id))
    });
    for (index, account) in accounts.iter_mut().enumerate() {
        account.latest = index == 0 && account.last_modified.is_some();
    }
}

/// What `--account` asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountChoice {
    UserId(i64),
    /// The account whose databases were written most recently.
    Latest,
}

impl std::str::FromStr for AccountChoice {
    type Err = String;

    fn from_str(raw: &str) -> std::result::Result<Self, Self::Err> {
        let raw = raw.trim();
        if raw.eq_ignore_ascii_case("latest") {
            return Ok(Self::Latest);
        }
        match raw.parse::<i64>() {
            Ok(id) if id > 0 => Ok(Self::UserId(id)),
            _ => Err(format!(
                "expected a KakaoTalk user id or \"latest\", got {raw:?}; run `lazykatok accounts`"
            )),
        }
    }
}

/// Pick the listed account `choice` names, or say why none matches.
pub fn choose_account(accounts: &[Account], choice: &AccountChoice) -> Result<Account> {
    let found = match choice {
        AccountChoice::Latest => accounts.iter().find(|account| account.latest),
        AccountChoice::UserId(id) => accounts.iter().find(|account| account.user_id == *id),
    };
    found.cloned().ok_or_else(|| {
        Error::Kakao(match choice {
            AccountChoice::Latest => "no KakaoTalk account opens a database on this Mac".to_string(),
            AccountChoice::UserId(id) => format!(
                "account {id} does not open any KakaoTalk database on this Mac; run `lazykatok accounts`"
            ),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pin(dir: &Path, user_id: i64) {
        std::fs::create_dir_all(dir.join("kakao")).expect("mkdir");
        std::fs::write(
            katok_cache_path(dir),
            format!(r#"{{"user_id": {user_id}, "uuid": "00000000-0000-0000-0000-000000000000"}}"#),
        )
        .expect("write cache");
    }

    fn account(user_id: i64, latest: bool, minute: u32) -> Account {
        Account {
            user_id,
            latest,
            databases: 1,
            last_modified: Some(
                chrono::TimeZone::with_ymd_and_hms(&Utc, 2026, 1, 1, 0, minute, 0).unwrap(),
            ),
            data_dir: PathBuf::from("/synthetic"),
        }
    }

    #[test]
    fn the_pinned_account_keeps_the_base_dir_and_others_get_their_own() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path();
        // Nothing pinned yet: the first account chosen takes the base dir.
        assert_eq!(account_data_dir(base, 1001), base);

        pin(base, 1001);
        assert_eq!(account_data_dir(base, 1001), base);
        assert_eq!(
            account_data_dir(base, 2002),
            base.join("accounts").join("2002")
        );
    }

    #[test]
    fn cached_ids_include_every_per_account_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path();
        pin(base, 1001);
        pin(&base.join("accounts").join("2002"), 2002);

        let mut ids = cached_user_ids(base);
        ids.sort_unstable();
        assert_eq!(ids, [1001, 2002]);
    }

    #[test]
    fn account_choice_parses_ids_and_latest() {
        assert_eq!("latest".parse(), Ok(AccountChoice::Latest));
        assert!("active".parse::<AccountChoice>().is_err());
        assert_eq!(" 2002 ".parse(), Ok(AccountChoice::UserId(2002)));
        assert!("0".parse::<AccountChoice>().is_err());
        assert!("someone".parse::<AccountChoice>().is_err());
    }

    #[test]
    fn choosing_finds_the_named_or_latest_account_and_refuses_others() {
        let accounts = [account(1001, false, 5), account(2002, true, 1)];

        assert_eq!(
            choose_account(&accounts, &AccountChoice::Latest)
                .unwrap()
                .user_id,
            2002
        );
        assert_eq!(
            choose_account(&accounts, &AccountChoice::UserId(1001))
                .unwrap()
                .user_id,
            1001
        );
        assert!(choose_account(&accounts, &AccountChoice::UserId(3003)).is_err());
        assert!(choose_account(&accounts[..1], &AccountChoice::Latest).is_err());
    }

    #[test]
    fn the_most_recently_written_account_sorts_first_and_is_the_latest() {
        let mut accounts = vec![
            account(1001, true, 1),
            account(3003, false, 9),
            account(2002, false, 0),
        ];
        sort_accounts(&mut accounts);
        let order: Vec<(i64, bool)> = accounts.iter().map(|a| (a.user_id, a.latest)).collect();
        assert_eq!(order, [(3003, true), (1001, false), (2002, false)]);
    }
}
