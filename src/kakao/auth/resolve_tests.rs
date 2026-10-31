//! Synthetic account stores only; no platform UUID lookup or live app data.

use super::*;
use rusqlite::Connection;
use sha2::{Digest, Sha512};
use std::time::{Duration, SystemTime};

const UUID: &str = "00000000-1111-2222-3333-444444444444";
const OLD: i64 = 11;
const ACTIVE: i64 = 22;

struct Accounts {
    _temp: tempfile::TempDir,
    options: AuthOptions,
    old_db: PathBuf,
    active_db: PathBuf,
}

fn set_modified(path: &Path, seconds: u64) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .expect("fixture file")
        .set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(seconds))
        .expect("fixture mtime");
}

impl Accounts {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("synthetic home");
        let options = AuthOptions {
            home: temp.path().join("home"),
            data_dir: temp.path().join("data"),
            user_id_override: None,
            uuid_override: None,
            max_user_id: 30,
        };
        let store = container_dir(&options.home);
        std::fs::create_dir_all(&store).expect("synthetic store");
        let create_db = |user_id, seconds| {
            let path = store.join(derive::database_name(user_id, UUID));
            let conn = Connection::open(&path).expect("synthetic database");
            let key = derive::secure_key(user_id, UUID);
            conn.execute_batch(&format!(
                "PRAGMA key = '{key}'; PRAGMA cipher_compatibility = 3;
                 CREATE TABLE synthetic (value INTEGER);"
            ))
            .expect("encrypted schema");
            drop(conn);
            set_modified(&path, seconds);
            path
        };
        let old_db = create_db(OLD, 1_000);
        let active_db = create_db(ACTIVE, 2_000);
        Self {
            _temp: temp,
            options,
            old_db,
            active_db,
        }
    }

    fn plist(&self, user_id: i64, direct: bool) -> PathBuf {
        let prefs = self
            .options
            .home
            .join("Library/Containers/com.kakao.KakaoTalkMac/Data/Library/Preferences");
        std::fs::create_dir_all(&prefs).expect("synthetic preferences");
        let stem = if user_id == OLD { "a" } else { "b" }.repeat(40);
        let path = prefs.join(format!("com.kakao.KakaoTalkMac.{stem}.plist"));
        let hash = super::super::hex_for_test(&Sha512::digest(user_id.to_string().as_bytes()));
        let candidate = if direct {
            format!("<key>userId</key><integer>{user_id}</integer>")
        } else {
            String::new()
        };
        std::fs::write(
            &path,
            format!(
                "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict>{candidate}\
                 <key>DESIGNATEDFRIENDSREVISION:{hash}</key><integer>1</integer>\
                 </dict></plist>"
            ),
        )
        .expect("synthetic plist");
        set_modified(&path, if user_id == OLD { 1_000 } else { 2_000 });
        path
    }

    fn resolve(&self) -> ResolvedAuth {
        resolve::resolve_with_uuid(&self.options, UUID.to_string())
            .expect("resolve synthetic account")
    }
}

#[test]
fn selects_active_account_when_both_plists_have_candidates() {
    // Given two valid accounts whose lexical and numeric order favors the old one.
    let accounts = Accounts::new();
    accounts.plist(OLD, true);
    accounts.plist(ACTIVE, true);
    // When resolving without an override.
    let resolved = accounts.resolve();
    // Then only the active account's database is returned.
    assert_eq!(resolved.user_id, ACTIVE);
    assert_eq!(resolved.database_files, vec![accounts.active_db]);
}

#[test]
fn recovers_active_account_when_plists_only_contain_hashes() {
    // Given two hash-only account preferences.
    let accounts = Accounts::new();
    accounts.plist(OLD, false);
    accounts.plist(ACTIVE, false);
    // When recovering an identity.
    let resolved = accounts.resolve();
    // Then the newer account is recovered.
    assert_eq!(resolved.user_id, ACTIVE);
}

#[test]
fn stale_caches_do_not_hide_a_newer_hash_only_account() {
    // Given either cache source retaining the old account.
    for katok_cache in [true, false] {
        let accounts = Accounts::new();
        accounts.plist(OLD, false);
        accounts.plist(ACTIVE, false);
        if katok_cache {
            persist_katok_cache(&katok_cache_path(&accounts.options.data_dir), OLD, UUID);
        } else {
            let path = k_skill_cache_path(&accounts.options.home);
            std::fs::create_dir_all(path.parent().expect("cache parent")).expect("cache directory");
            std::fs::write(path, format!("{{\"user_id\":{OLD}}}")).expect("bootstrap cache");
        }
        // When resolving against both encrypted databases.
        let resolved = accounts.resolve();
        // Then the stale cache is replaced with the active identity.
        assert_eq!(resolved.user_id, ACTIVE);
        assert_eq!(
            read_katok_cache(&katok_cache_path(&accounts.options.data_dir)),
            Some((ACTIVE, UUID.to_string()))
        );
    }
}

#[test]
fn newest_openable_database_wins_over_preference_mtime() {
    // Given an old account preference touched after the active account's.
    let accounts = Accounts::new();
    let old = accounts.plist(OLD, true);
    accounts.plist(ACTIVE, true);
    set_modified(&old, 3_000);
    // When selecting the account.
    let resolved = accounts.resolve();
    // Then database activity wins.
    assert_eq!(resolved.user_id, ACTIVE);
}

#[test]
fn recovery_continues_when_the_newest_hash_has_no_openable_database() {
    // Given the newest preference belongs to an account without a database.
    let accounts = Accounts::new();
    accounts.plist(OLD, false);
    accounts.plist(ACTIVE, false);
    std::fs::remove_file(&accounts.active_db).expect("remove synthetic account database");
    // When hash recovery reaches an unopenable identity.
    let resolved = accounts.resolve();
    // Then an older working identity remains usable.
    assert_eq!(resolved.user_id, OLD);
}

#[test]
fn explicit_override_selects_an_older_account() {
    // Given automatic selection would prefer the newer account.
    let mut accounts = Accounts::new();
    accounts.plist(OLD, true);
    accounts.plist(ACTIVE, true);
    accounts.options.user_id_override = Some(OLD);
    // When an explicit identity is requested.
    let resolved = accounts.resolve();
    // Then it remains authoritative.
    assert_eq!(resolved.user_id, OLD);
    assert_eq!(resolved.source, "override");
}

#[test]
fn single_account_cache_keeps_its_existing_precedence() {
    // Given one account with a valid cached identity.
    let accounts = Accounts::new();
    accounts.plist(OLD, false);
    std::fs::remove_file(&accounts.active_db).expect("remove second synthetic account");
    persist_katok_cache(&katok_cache_path(&accounts.options.data_dir), OLD, UUID);
    // When resolving.
    let resolved = accounts.resolve();
    // Then cache resolution is unchanged.
    assert_eq!(resolved.user_id, OLD);
    assert_eq!(resolved.source, "katok-cache");
}

#[test]
fn committed_wal_activity_selects_the_active_account() {
    // Given the old account's base file is newer, but the active account writes a WAL.
    let accounts = Accounts::new();
    accounts.plist(OLD, true);
    accounts.plist(ACTIVE, true);
    set_modified(&accounts.old_db, 3_000);
    let conn = Connection::open(&accounts.active_db).expect("synthetic writer");
    let key = derive::secure_key(ACTIVE, UUID);
    conn.execute_batch(&format!(
        "PRAGMA key = '{key}'; PRAGMA cipher_compatibility = 3;
         PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0;
         INSERT INTO synthetic VALUES (1);"
    ))
    .expect("committed WAL");
    let wal = PathBuf::from(format!("{}-wal", accounts.active_db.display()));
    set_modified(&wal, 4_000);
    // When resolving while the writer remains open.
    let resolved = accounts.resolve();
    // Then the committed activity determines the identity.
    assert_eq!(resolved.user_id, ACTIVE);
}
