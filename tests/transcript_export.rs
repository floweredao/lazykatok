//! Transcript export has to be safe to re-run: the old Python capture it replaces could clobber
//! an earlier capture, and a run that found nothing new had to leave existing files alone.

use lazykatok::{archive::Archive, transcript::export_transcript, types::RawMessage};

fn message(id: &str, chat: &str, seconds: i64, text: &str) -> RawMessage {
    message_of_type(id, chat, seconds, text, "text")
}

fn message_of_type(
    id: &str,
    chat: &str,
    seconds: i64,
    text: &str,
    message_type: &str,
) -> RawMessage {
    let base = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .expect("base timestamp")
        .with_timezone(&chrono::Utc);
    RawMessage {
        account_hash: "acct".to_string(),
        chat_id: chat.to_string(),
        chat_name: "테스트방".to_string(),
        chat_type: "group".to_string(),
        message_id: id.to_string(),
        sender_id: "u1".to_string(),
        sender_nickname: "합성A".to_string(),
        timestamp: base + chrono::Duration::seconds(seconds),
        text: text.to_string(),
        message_type: message_type.to_string(),
        reply_to_message_id: None,
    }
}

fn archive_with(messages: &[RawMessage]) -> (tempfile::TempDir, Archive) {
    let dir = tempfile::tempdir().expect("tempdir");
    let archive = Archive::open(&dir.path().join("archive.sqlite3")).expect("open archive");
    archive.sync_messages(messages).expect("sync");
    (dir, archive)
}

#[test]
fn exporting_the_same_range_twice_writes_identical_bytes() {
    let messages = vec![
        message("m001", "A", 0, "첫 메시지"),
        message("m002", "A", 60, "둘째 메시지"),
    ];
    let (dir, archive) = archive_with(&messages);
    let out = dir.path().join("out");

    let first = export_transcript(&archive, "A", None, &out).expect("export");
    let path = first.path.clone().expect("a file for a non-empty range");
    let first_bytes = std::fs::read(&path).expect("read transcript");

    let second = export_transcript(&archive, "A", None, &out).expect("re-export");
    assert_eq!(second.path.as_ref(), Some(&path), "same range, same file");
    assert_eq!(
        first_bytes,
        std::fs::read(&path).expect("read transcript again"),
        "re-export changed the file"
    );
    assert_eq!(second.messages, 2);
}

/// An output directory the caller already had is theirs. The transcript lands in it owner-only,
/// and the directory keeps the permissions it came with: tightening it would change how every
/// other user and process sees a path lazykatok was only asked to write into.
#[cfg(unix)]
#[test]
fn an_existing_output_directory_keeps_its_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let messages = vec![message("m001", "A", 0, "첫 메시지")];
    let (dir, archive) = archive_with(&messages);
    let existing = dir.path().join("shared");
    std::fs::create_dir(&existing).expect("create output dir");
    std::fs::set_permissions(&existing, std::fs::Permissions::from_mode(0o755))
        .expect("set output dir mode");

    let report = export_transcript(&archive, "A", None, &existing).expect("export");

    let mode = |path: &std::path::Path| {
        std::fs::metadata(path)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(
        mode(&existing),
        0o755,
        "the directory the caller named was re-permissioned"
    );
    assert_eq!(
        mode(&report.path.expect("a file for a non-empty range")),
        0o600,
        "the transcript itself must stay owner-only"
    );
}

/// A directory the export creates holds raw message bodies, so it is created owner-only.
#[cfg(unix)]
#[test]
fn an_output_directory_the_export_creates_is_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let messages = vec![message("m001", "A", 0, "첫 메시지")];
    let (dir, archive) = archive_with(&messages);
    let fresh = dir.path().join("new").join("nested");

    export_transcript(&archive, "A", None, &fresh).expect("export");

    let mode = std::fs::metadata(&fresh)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o700);
}

#[test]
fn a_range_with_no_messages_writes_nothing_and_leaves_earlier_exports_alone() {
    let messages = vec![message("m001", "A", 0, "첫 메시지")];
    let (dir, archive) = archive_with(&messages);
    let out = dir.path().join("out");

    let first = export_transcript(&archive, "A", None, &out).expect("export");
    let path = first.path.clone().expect("a file");
    let before = std::fs::read(&path).expect("read transcript");

    // Everything in this archive predates the cutoff.
    let empty = export_transcript(&archive, "A", Some("2030-01-01T00:00:00+00:00"), &out)
        .expect("export empty range");
    assert!(empty.path.is_none(), "an empty range must write no file");
    assert_eq!(empty.messages, 0);
    assert_eq!(
        before,
        std::fs::read(&path).expect("earlier transcript still readable"),
        "an empty run overwrote an earlier export"
    );
}

#[test]
fn a_later_range_lands_in_its_own_file() {
    let messages = vec![
        message("m001", "A", 0, "첫 메시지"),
        message("m002", "A", 600, "나중 메시지"),
    ];
    let (dir, archive) = archive_with(&messages);
    let out = dir.path().join("out");

    let all = export_transcript(&archive, "A", None, &out).expect("export all");
    let tail = export_transcript(&archive, "A", Some("2026-01-01T00:05:00+00:00"), &out)
        .expect("export tail");

    assert_ne!(
        all.path, tail.path,
        "a different range must not reuse the same file name"
    );
    assert_eq!(all.messages, 2);
    assert_eq!(tail.messages, 1);
    assert!(all.path.expect("path").exists(), "earlier export survived");
}

#[test]
fn system_feed_messages_are_left_out_of_the_transcript_but_stay_in_the_archive() {
    let messages = vec![
        message("m001", "A", 0, "사람이 쓴 말"),
        message_of_type(
            "m002",
            "A",
            30,
            r#"{"feedType":4,"member":{"nickName":"합성B"}}"#,
            "type_0",
        ),
        message("m003", "A", 60, "또 사람이 쓴 말"),
    ];
    let (dir, archive) = archive_with(&messages);
    let out = dir.path().join("out");

    let report = export_transcript(&archive, "A", None, &out).expect("export");
    assert_eq!(report.messages, 2);
    assert_eq!(report.feed_skipped, 1);

    let body = std::fs::read_to_string(report.path.expect("path")).expect("read transcript");
    assert!(body.contains("사람이 쓴 말"));
    assert!(
        !body.contains("feedType"),
        "system feed leaked into the transcript"
    );

    // The archive keeps the raw row: filtering is a presentation choice, not deletion.
    let stored = archive
        .messages_for_transcript("A", None)
        .expect("read archive");
    assert_eq!(stored.len(), 3);
}

#[test]
fn user_authored_json_with_membership_keys_stays_in_the_transcript() {
    let messages = vec![
        message(
            "m001",
            "A",
            0,
            r#"{"member":"ordinary user-authored JSON"}"#,
        ),
        message_of_type(
            "m002",
            "A",
            30,
            r#"{"feedType":4,"member":{"nickName":"synthetic"}}"#,
            "type_0",
        ),
    ];
    let (dir, archive) = archive_with(&messages);
    let report = export_transcript(&archive, "A", None, &dir.path().join("out")).expect("export");

    assert_eq!(report.messages, 1);
    assert_eq!(report.feed_skipped, 1);
    let body = std::fs::read_to_string(report.path.expect("path")).expect("read transcript");
    assert!(
        body.contains("ordinary user-authored JSON"),
        "a text message must not be classified as a system feed from its body alone"
    );
}

#[test]
fn since_written_with_z_includes_a_message_sent_at_that_instant() {
    // The archive stores `+00:00`; a `Z` cutoff for the same instant compared as text sorted
    // after it, dropping the boundary message the help promises ("at or after").
    let messages = vec![
        message("m001", "A", 0, "경계 이전"),
        message("m002", "A", 300, "경계 메시지"),
        message("m003", "A", 360, "경계 이후"),
    ];
    let (dir, archive) = archive_with(&messages);

    let report = export_transcript(
        &archive,
        "A",
        Some("2026-01-01T00:05:00Z"),
        &dir.path().join("out"),
    )
    .expect("export");

    assert_eq!(report.messages, 2);
    assert_eq!(report.first_message_id.as_deref(), Some("m002"));
}

#[test]
fn since_with_a_non_utc_offset_is_compared_as_an_instant() {
    // 09:05 at +09:00 is 00:05 UTC, so only the last two messages are at or after it.
    let messages = vec![
        message("m001", "A", 0, "이전"),
        message("m002", "A", 300, "경계"),
        message("m003", "A", 3_600, "한 시간 뒤"),
    ];
    let (dir, archive) = archive_with(&messages);

    let report = export_transcript(
        &archive,
        "A",
        Some("2026-01-01T09:05:00+09:00"),
        &dir.path().join("out"),
    )
    .expect("export");

    assert_eq!(report.messages, 2);
    assert_eq!(report.first_message_id.as_deref(), Some("m002"));
}

#[test]
fn since_that_is_not_rfc3339_is_rejected_instead_of_matching_nothing() {
    let messages = vec![message("m001", "A", 0, "메시지")];
    let (dir, archive) = archive_with(&messages);
    let out = dir.path().join("out");

    for since in ["yesterday", "2026-01-01", "2026-01-01 00:00:00"] {
        assert!(
            export_transcript(&archive, "A", Some(since), &out).is_err(),
            "{since:?} must be refused, not compared as text"
        );
    }
    assert!(!out.exists(), "a refused range must write nothing");
}
