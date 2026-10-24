//! Korean and other non-ASCII text through search and chunking. Synthetic fixtures only.

use assert_cmd::Command;
use lazykatok::{
    archive::Archive,
    chunking::rebuild_chunks,
    fixture::read_fixture,
    search::{bm25_search, keyword_search},
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
