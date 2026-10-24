//! Korean and other non-ASCII text through search and chunking. Synthetic fixtures only.

use assert_cmd::Command;
use lazykatok::{
    archive::Archive,
    chunking::rebuild_chunks,
    fixture::read_fixture,
    search::{bm25_search, keyword_search},
    types::RawMessage,
};
use serde_json::Value;
use std::path::Path;
use unicode_normalization::UnicodeNormalization;

fn fixture_path() -> String {
    format!(
        "{}/tests/fixtures/kakao/korean.jsonl",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn chunked_archive(dir: &Path) -> Archive {
    let archive = Archive::open(&dir.join("archive.sqlite3")).expect("open archive");
    let messages = read_fixture(fixture_path()).expect("read fixture");
    archive.sync_messages(&messages).expect("sync messages");
    rebuild_chunks(&archive).expect("rebuild chunks");
    archive
}

/// The form macOS hands out for file names and text copied from them.
fn decomposed(text: &str) -> String {
    let nfd: String = text.nfd().collect();
    assert_ne!(nfd, text, "fixture query must change under NFD");
    nfd
}

#[test]
fn bm25_finds_a_hangul_word_with_a_particle_attached() {
    // Korean attaches particles to the word (`회의는`), and the FTS tokenizer only splits on
    // spaces and punctuation, so the bare noun has to match the start of a token.
    let dir = tempfile::tempdir().expect("tempdir");
    let archive = chunked_archive(dir.path());

    let hits = bm25_search(&archive, "회의", 10).expect("bm25 search");

    assert_eq!(hits.len(), 1, "{hits:?}");
    assert!(hits[0].snippet.contains("회의는"), "{}", hits[0].snippet);
}

#[test]
fn keyword_and_bm25_match_a_decomposed_query() {
    let dir = tempfile::tempdir().expect("tempdir");
    let archive = chunked_archive(dir.path());

    let keyword = keyword_search(&archive, &decomposed("회의"), 10).expect("keyword search");
    assert_eq!(keyword.len(), 1, "{keyword:?}");
    assert!(keyword[0].snippet.contains("회의는"));

    let bm25 = bm25_search(&archive, &decomposed("보고서"), 10).expect("bm25 search");
    assert_eq!(bm25.len(), 1, "{bm25:?}");
    assert!(bm25[0].snippet.contains("보고서"));
}

fn lazykatok(data_dir: &Path) -> Command {
    let mut command = Command::cargo_bin("lazykatok").expect("lazykatok binary");
    command
        .env("KATOK_EMBEDDER", "local-test")
        .args(["--data-dir", data_dir.to_str().expect("utf8 path")]);
    command
}

fn search(data_dir: &Path, mode: &str, query: &str) -> Vec<Value> {
    let output = lazykatok(data_dir)
        .args(["search", mode, query, "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice::<Vec<Value>>(&output).expect("search emits a JSON array")
}

#[test]
fn semantic_and_hybrid_match_a_decomposed_query() {
    let dir = tempfile::tempdir().expect("tempdir");
    lazykatok(dir.path())
        .args(["sync", "--source", "fixture", &fixture_path(), "--json"])
        .assert()
        .success();
    lazykatok(dir.path())
        .args(["index", "--json"])
        .assert()
        .success();

    let query = decomposed("점심");
    for mode in ["semantic", "hybrid"] {
        let hits = search(dir.path(), mode, &query);
        assert!(!hits.is_empty(), "{mode}: no hits");
        let snippet = hits[0]["snippet"].as_str().expect("snippet");
        assert!(snippet.contains("점심"), "{mode}: {snippet}");
    }
}

#[test]
fn parent_windows_are_not_cut_inside_a_grapheme_cluster() {
    // A regional-indicator flag, a ZWJ emoji sequence, and a decomposed Hangul syllable are
    // each one visible character spread over several code points.
    let clusters = [
        "\u{1F1F0}\u{1F1F7}",
        "\u{1F469}\u{200D}\u{1F469}\u{200D}\u{1F467}",
        "\u{1112}\u{1161}\u{11AB}",
    ];
    for cluster in clusters {
        let dir = tempfile::tempdir().expect("tempdir");
        let archive = Archive::open(&dir.path().join("archive.sqlite3")).expect("open archive");
        // The parent line is `[가람] ` (5 chars) plus the text; the window limit is 3000
        // chars, so the cluster starts one char before the cut.
        let text = format!("{}{cluster}끝", "가".repeat(2999 - 5));
        let message = RawMessage {
            account_hash: "acct-synthetic".to_string(),
            chat_id: "chat-long".to_string(),
            chat_name: "Synthetic Long Room".to_string(),
            chat_type: "group".to_string(),
            message_id: "long-1".to_string(),
            sender_id: "u1".to_string(),
            sender_nickname: "가람".to_string(),
            timestamp: "2026-01-01T09:00:00Z".parse().expect("timestamp"),
            text,
            message_type: "text".to_string(),
            reply_to_message_id: None,
        };
        archive.sync_messages(&[message]).expect("sync messages");
        rebuild_chunks(&archive).expect("rebuild chunks");

        let parents = archive.all_parent_chunks().expect("parent chunks");
        assert_eq!(parents.len(), 2, "{cluster:?}");
        let (head, tail): (Vec<_>, Vec<_>) = parents
            .iter()
            .partition(|parent| parent.text.starts_with("[가람] "));
        assert!(
            head[0].text.ends_with('가'),
            "{cluster:?}: first window ends inside the cluster"
        );
        assert!(
            tail[0].text.starts_with(cluster),
            "{cluster:?}: second window starts inside the cluster"
        );
    }
}
