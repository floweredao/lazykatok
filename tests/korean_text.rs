//! Korean and other non-ASCII text through search and chunking. Synthetic fixtures only.

use lazykatok::{
    archive::Archive, chunking::rebuild_chunks, fixture::read_fixture, search::bm25_search,
};
use std::path::Path;

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
