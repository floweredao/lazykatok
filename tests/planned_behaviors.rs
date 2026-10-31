use lazykatok::{
    archive::Archive,
    chunking::rebuild_chunks,
    fixture::read_fixture,
    search::{bm25_search, keyword_search},
    semantic::{
        planned_semantic_documents_for_parents, semantic_search, write_semantic_documents,
        write_semantic_documents_for_parents,
    },
    types::RawMessage,
};

#[test]
fn semantic_documents_accept_a_loaded_parent_snapshot() {
    let dir = tempfile::tempdir().expect("tempdir");
    let archive_path = dir.path().join("archive.sqlite3");
    let archive = Archive::open(&archive_path).expect("open archive");
    let fixture_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/kakao/replies.jsonl");
    let messages = read_fixture(&fixture_path).expect("read fixture");

    archive.sync_messages(&messages).expect("sync messages");
    rebuild_chunks(&archive).expect("rebuild chunks");
    let parents = archive.all_parent_chunks().expect("load parents");
    let semantic_dir = dir.path().join("semantic");

    let planned = planned_semantic_documents_for_parents(&parents, &semantic_dir);
    let written = write_semantic_documents_for_parents(&parents, &semantic_dir)
        .expect("write semantic documents");

    assert_eq!(planned.len(), parents.len());
    assert_eq!(written, parents.len());
    assert_eq!(planned[0].chunk_id, parents[0].parent_id);
    assert!(planned.iter().all(|document| document.path.exists()));
}

#[test]
fn same_sender_reply_and_search_behaviors_when_fixture_is_indexed() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let archive_path = dir.path().join("archive.sqlite3");
    let archive = Archive::open(&archive_path).expect("open archive");
    let fixture = format!(
        "{}/tests/fixtures/kakao/replies.jsonl",
        env!("CARGO_MANIFEST_DIR")
    );
    let messages = read_fixture(&fixture).expect("read fixture");

    archive.sync_messages(&messages).expect("sync messages");
    rebuild_chunks(&archive).expect("rebuild chunks");
    write_semantic_documents(&archive, &dir.path().join("semantic"))
        .expect("write semantic documents");

    let child = archive
        .get_chunk("chunk_caaaca07be83adf8")
        .expect("get chunk")
        .expect("known child chunk");
    assert_eq!(child.parent_chunk_ids, vec!["chunk_2aeac4db0a04ceb2"]);
    assert_eq!(child.message_count, 1);

    let keyword = keyword_search(&archive, "보고서", 10).expect("keyword search");
    assert_eq!(keyword[0].chunk_id, "chunk_2aeac4db0a04ceb2");
    assert!(keyword[0].snippet.chars().count() <= 80);

    let bm25 = bm25_search(&archive, "보고서", 10).expect("bm25 search");
    assert_eq!(bm25[0].ranker, "bm25");

    let semantic = semantic_search(&archive, &dir.path().join("semantic"), "회의 보고서", 10)
        .expect("semantic search");
    assert_eq!(semantic[0].unit, "parent_window");
    assert!(semantic[0]
        .child_chunk_ids
        .contains(&"chunk_2aeac4db0a04ceb2".to_string()));
}

#[test]
fn parent_windows_group_same_chat_messages_across_senders_when_fixture_is_indexed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let archive_path = dir.path().join("archive.sqlite3");
    let archive = Archive::open(&archive_path).expect("open archive");
    let fixture_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/kakao/replies.jsonl");
    let messages = read_fixture(&fixture_path).expect("read fixture");

    archive.sync_messages(&messages).expect("sync messages");
    rebuild_chunks(&archive).expect("rebuild chunks");

    let child = archive
        .get_chunk("chunk_caaaca07be83adf8")
        .expect("get child")
        .expect("known child");
    assert_eq!(child.window_parent_ids.len(), 1);

    let parent = archive
        .get_parent_chunk(&child.window_parent_ids[0])
        .expect("get parent")
        .expect("known parent");
    assert_eq!(parent.child_chunk_ids.len(), 2);
    assert!(parent.text.contains("[민지] 보고서 초안 올렸어요"));
    assert!(parent.text.contains("[준호] 회의 전에 확인할게요"));
    assert!(parent.text.len() <= lazykatok::chunking::DEFAULT_PARENT_WINDOW_MAX_CHARS);
}

#[test]
fn parent_windows_cap_single_large_child_when_indexed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let archive_path = dir.path().join("archive.sqlite3");
    let archive = Archive::open(&archive_path).expect("open archive");
    let long_text = format!(
        "{}꼬리검색",
        "가".repeat(lazykatok::chunking::DEFAULT_PARENT_WINDOW_MAX_CHARS + 256)
    );
    let message = RawMessage {
        account_hash: "acct".to_string(),
        chat_id: "chat-large".to_string(),
        chat_name: "Large Fixture".to_string(),
        chat_type: "group".to_string(),
        message_id: "large-1".to_string(),
        sender_id: "sender-1".to_string(),
        sender_nickname: "민지".to_string(),
        timestamp: chrono::DateTime::parse_from_rfc3339("2026-01-01T09:00:00Z")
            .expect("timestamp")
            .with_timezone(&chrono::Utc),
        text: long_text,
        message_type: "text".to_string(),
        reply_to_message_id: None,
    };

    archive.sync_messages(&[message]).expect("sync message");
    rebuild_chunks(&archive).expect("rebuild chunks");

    let parents = archive.all_parent_chunks().expect("load parents");
    assert!(parents.len() > 1);
    assert!(parents.iter().all(|parent| {
        parent.text.chars().count() <= lazykatok::chunking::DEFAULT_PARENT_WINDOW_MAX_CHARS
    }));
    assert!(parents
        .iter()
        .map(|parent| parent.text.as_str())
        .collect::<Vec<_>>()
        .join("")
        .contains("꼬리검색"));

    write_semantic_documents(&archive, &dir.path().join("semantic"))
        .expect("write semantic documents");
    let semantic = semantic_search(&archive, &dir.path().join("semantic"), "꼬리검색", 10)
        .expect("semantic search");
    let child_id = archive.all_chunks().expect("chunks")[0].chunk_id.clone();
    assert!(semantic[0].child_chunk_ids.contains(&child_id));
}

#[test]
fn semantic_document_rebuild_prunes_stale_parent_markdown_when_indexed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let archive_path = dir.path().join("archive.sqlite3");
    let archive = Archive::open(&archive_path).expect("open archive");
    let fixture_path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/kakao/replies.jsonl");
    let messages = read_fixture(&fixture_path).expect("read fixture");

    archive.sync_messages(&messages).expect("sync messages");
    rebuild_chunks(&archive).expect("rebuild chunks");
    let semantic_dir = dir.path().join("semantic");
    let source_dir = semantic_dir.join("source").join("chunks");
    std::fs::create_dir_all(&source_dir).expect("create source dir");
    std::fs::write(source_dir.join("window_stale.md"), "stale searchable tail")
        .expect("write stale doc");

    write_semantic_documents(&archive, &semantic_dir).expect("write semantic documents");

    assert!(!source_dir.join("window_stale.md").exists());
    let stale = semantic_search(&archive, &semantic_dir, "stale", 10).expect("semantic search");
    assert!(stale.is_empty());
}

#[test]
fn keyword_search_keeps_the_newest_matches_when_there_are_more_than_the_limit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let archive = Archive::open(&dir.path().join("archive.sqlite3")).expect("open archive");
    let message = |day: u32| RawMessage {
        account_hash: "acct".to_string(),
        chat_id: "chat-limit".to_string(),
        chat_name: "Limit Fixture".to_string(),
        chat_type: "group".to_string(),
        message_id: format!("lim-{day}"),
        sender_id: "sender-1".to_string(),
        sender_nickname: "합성A".to_string(),
        timestamp: chrono::DateTime::parse_from_rfc3339(&format!("2026-02-{day:02}T09:00:00Z"))
            .expect("timestamp")
            .with_timezone(&chrono::Utc),
        text: format!("예정된 일정 항목 {day}"),
        message_type: "text".to_string(),
        reply_to_message_id: None,
    };
    let messages = (1..=12).map(message).collect::<Vec<_>>();
    archive.sync_messages(&messages).expect("sync messages");
    rebuild_chunks(&archive).expect("rebuild chunks");
    assert_eq!(archive.all_chunks().expect("chunks").len(), 12);

    let hits = keyword_search(&archive, "일정", 3).expect("keyword search");

    let snippets = hits
        .iter()
        .map(|hit| hit.snippet.as_str())
        .collect::<Vec<_>>();
    assert_eq!(hits.len(), 3);
    assert!(
        snippets.iter().any(|snippet| snippet.contains("항목 12")),
        "the limit must keep the newest matches, got {snippets:?}"
    );
    assert!(
        !snippets.iter().any(|snippet| snippet.ends_with("항목 1")),
        "the oldest match must not survive the limit, got {snippets:?}"
    );
}

#[test]
fn keyword_search_treats_percent_and_underscore_as_literal_text() {
    let dir = tempfile::tempdir().expect("tempdir");
    let archive = Archive::open(&dir.path().join("archive.sqlite3")).expect("open archive");
    let message = |id: &str, hour: u32, text: &str| RawMessage {
        account_hash: "acct".to_string(),
        chat_id: "chat-literal".to_string(),
        chat_name: "Literal Fixture".to_string(),
        chat_type: "group".to_string(),
        message_id: id.to_string(),
        sender_id: "sender-1".to_string(),
        sender_nickname: "합성A".to_string(),
        timestamp: chrono::DateTime::parse_from_rfc3339(&format!("2026-01-01T{hour:02}:00:00Z"))
            .expect("timestamp")
            .with_timezone(&chrono::Utc),
        text: text.to_string(),
        message_type: "text".to_string(),
        reply_to_message_id: None,
    };
    // Hours apart, so each message is its own chunk.
    archive
        .sync_messages(&[
            message("lit-1", 9, "할인 50% 적용"),
            message("lit-2", 12, "할인 500원 적용"),
            message("lit-3", 15, "파일 report_v2 공유"),
            message("lit-4", 18, "파일 reportXv2 공유"),
            message("lit-5", 21, r"경로 C:\temp 확인"),
        ])
        .expect("sync messages");
    rebuild_chunks(&archive).expect("rebuild chunks");
    assert_eq!(archive.all_chunks().expect("chunks").len(), 5);

    let percent = keyword_search(&archive, "50%", 10).expect("keyword search");
    assert_eq!(percent.len(), 1, "`%` must not act as a wildcard");
    assert!(percent[0].snippet.contains("50%"));

    let underscore = keyword_search(&archive, "report_v2", 10).expect("keyword search");
    assert_eq!(underscore.len(), 1, "`_` must not act as a wildcard");
    assert!(underscore[0].snippet.contains("report_v2"));

    // The escape character itself must still match literally.
    let backslash = keyword_search(&archive, r"C:\temp", 10).expect("keyword search");
    assert_eq!(backslash.len(), 1);
}
