
use super::*;

#[test]
fn extracts_platform_uuid_from_ioreg_block() {
    let sample = r#"  +-o IOPlatformExpertDevice  <class IOPlatformExpertDevice>
    {
      "IOPlatformUUID" = "00000000-1111-2222-3333-444444444444"
      "IOPlatformSerialNumber" = "XXXX"
    }"#;
    assert_eq!(
        extract_platform_uuid(sample).as_deref(),
        Some("00000000-1111-2222-3333-444444444444")
    );
}

#[test]
fn katok_cache_with_a_non_ascii_uuid_is_ignored() {
    // Twelve 3-byte characters are 36 bytes, the length of a real
    // IOPlatformUUID, but byte offsets 5 and 7 fall inside a character.
    let dir = tempfile::tempdir().expect("temp dir");
    let cache = dir.path().join("auth.json");
    std::fs::write(
        &cache,
        r#"{"user_id": 1000000001, "uuid": "가나다라마바사아자차카타"}"#,
    )
    .expect("cache");

    // The same steps resolve_auth takes with a cached identity.
    let verified = read_katok_cache(&cache).and_then(|(user_id, uuid)| verify(user_id, &uuid, &[]));
    assert_eq!(verified, None);
}

#[test]
fn recovers_small_user_id_via_sha512() {
    let target = {
        use sha2::{Digest, Sha512};
        let digest = Sha512::digest(b"4242");
        super::super::hex_for_test(&digest)
    };
    assert_eq!(recover_user_id_from_sha512(&target, 10_000), Some(4242));
}

#[test]
fn uppercase_active_hash_is_rejected() {
    // SHA-512 hexdigests are always lowercase; an uppercase hash must be
    // rejected before it can drive a fruitless full scan.
    let upper = "A".repeat(128);
    assert!(!is_active_account_hash(&upper, "integer", "5"));
    let lower = "a".repeat(128);
    assert!(is_active_account_hash(&lower, "integer", "5"));
}

#[test]
fn uppercase_hash_recovery_returns_without_scanning() {
    // An uppercase target can never match the lowercase digest comparison,
    // so it must short-circuit to None rather than scan 0..=max_user_id.
    // A huge max proves no scan ran (the test would hang otherwise).
    let upper = "A".repeat(128);
    assert_eq!(recover_user_id_from_sha512(&upper, i64::MAX), None);
}

#[test]
fn probe_status_reports_presence_only() {
    let dir = tempfile::tempdir().expect("temp dir");
    let home = dir.path().join("home");
    let data_dir = dir.path().join("data");

    // Nothing present yet.
    let empty = probe_status(&home, &data_dir);
    assert!(!empty.app_installed);
    assert!(!empty.container_present);
    assert_eq!(empty.db_file_count, 0);
    assert!(!empty.auth_cached);

    // Create the container with one valid DB file and an auth cache.
    let container = container_dir(&home);
    std::fs::create_dir_all(&container).expect("container");
    std::fs::write(container.join("a".repeat(78)), b"x").expect("db");
    std::fs::write(container.join("not-a-db.txt"), b"x").expect("noise");
    let cache = katok_cache_path(&data_dir);
    std::fs::create_dir_all(cache.parent().unwrap()).expect("cache dir");
    std::fs::write(&cache, b"{}").expect("cache");

    let ready = probe_status(&home, &data_dir);
    assert!(ready.app_installed);
    assert!(ready.container_present);
    assert_eq!(ready.db_file_count, 1);
    assert!(ready.auth_cached);
}

#[test]
fn scans_direct_user_id_key() {
    let xml = "<plist><dict><key>userId</key><integer>1000000001</integer></dict></plist>";
    let (ids, hash) = scan_plist_xml(xml);
    assert_eq!(ids, vec![1_000_000_001]);
    assert!(hash.is_none());
}

#[test]
fn scans_alert_array_and_active_hash() {
    let active = "a".repeat(128);
    let xml = format!(
            "<plist><dict>\
             <key>AlertKakaoIDsList</key><array><integer>111</integer><integer>222</integer></array>\
             <key>DESIGNATEDFRIENDSREVISION:{active}</key><integer>5</integer>\
             </dict></plist>"
        );
    let (ids, hash) = scan_plist_xml(&xml);
    assert_eq!(ids, vec![111, 222]);
    assert_eq!(hash.as_deref(), Some(active.as_str()));
}

#[test]
fn scans_active_hash_with_a_boolean_value() {
    // `plutil -convert xml1` writes booleans as self-closing `<true/>`.
    let active = "b".repeat(128);
    let xml =
        format!("<plist><dict><key>DESIGNATEDFRIENDSREVISION:{active}</key><true/></dict></plist>");
    let (_ids, hash) = scan_plist_xml(&xml);
    assert_eq!(hash.as_deref(), Some(active.as_str()));

    let inactive = format!(
        "<plist><dict><key>DESIGNATEDFRIENDSREVISION:{active}</key><false/></dict></plist>"
    );
    assert!(scan_plist_xml(&inactive).1.is_none());
}

#[test]
fn ignores_empty_account_hash() {
    let xml = format!(
            "<plist><dict><key>DESIGNATEDFRIENDSREVISION:{EMPTY_ACCOUNT_HASH}</key><integer>5</integer></dict></plist>"
        );
    let (_ids, hash) = scan_plist_xml(&xml);
    assert!(hash.is_none());
}
