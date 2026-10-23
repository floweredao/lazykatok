//! Where one chunk ends and the next begins, and the order chunks are read back in.

use chrono::{DateTime, TimeZone, Utc};
use lazykatok::{archive::Archive, chunking::rebuild_chunks, types::RawMessage};

fn at(second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 9, 0, second)
        .single()
        .expect("valid timestamp")
}

fn message(
    message_id: &str,
    sender_id: &str,
    sender_nickname: &str,
    second: u32,
    text: &str,
) -> RawMessage {
    RawMessage {
        account_hash: "acct-synthetic".to_string(),
        chat_id: "chat-synthetic".to_string(),
        chat_name: "합성 방".to_string(),
        chat_type: "group".to_string(),
        message_id: message_id.to_string(),
        sender_id: sender_id.to_string(),
        sender_nickname: sender_nickname.to_string(),
        timestamp: at(second),
        text: text.to_string(),
        message_type: "text".to_string(),
        reply_to_message_id: None,
    }
}

fn archive_with(messages: &[RawMessage]) -> (tempfile::TempDir, Archive) {
    let dir = tempfile::tempdir().expect("tempdir");
    let archive = Archive::open(&dir.path().join("archive.sqlite3")).expect("open archive");
    archive.sync_messages(messages).expect("sync");
    rebuild_chunks(&archive).expect("rebuild chunks");
    (dir, archive)
}

fn chunk_messages(archive: &Archive) -> Vec<Vec<String>> {
    archive
        .chunks_for_chat("chat-synthetic")
        .expect("chunks")
        .iter()
        .map(|summary| {
            archive
                .get_chunk(&summary.chunk_id)
                .expect("get chunk")
                .expect("chunk exists")
                .message_ids
        })
        .collect()
}

/// A chunk is one author's run. Display names are not unique in a group — two members can pick
/// the same nickname, and a source that has no name for a sender leaves it empty — so a run
/// keyed on the nickname alone filed two people's lines under whichever of them spoke first.
#[test]
fn different_senders_sharing_a_nickname_get_separate_chunks() {
    for nickname in ["합성이름", ""] {
        let (_dir, archive) = archive_with(&[
            message("m1", "sender-a", nickname, 0, "첫 번째 사람"),
            message("m2", "sender-b", nickname, 10, "두 번째 사람"),
            message("m3", "sender-b", nickname, 20, "두 번째 사람 이어서"),
        ]);

        assert_eq!(
            chunk_messages(&archive),
            vec![
                vec!["m1".to_string()],
                vec!["m2".to_string(), "m3".to_string()]
            ],
            "nickname {nickname:?}"
        );
    }
}

/// KakaoTalk stamps messages to the second, so in a busy room several authors' chunks start in
/// the same second. Those ties were broken by `chunk_id`, a content hash, which shuffled them:
/// `chunks` listed them out of order and `chunk context` skipped a neighbour on one side and
/// repeated it on the other.
#[test]
fn chunks_starting_in_the_same_second_keep_reading_order() {
    let senders = ["a", "b", "c", "d", "e", "f"];
    let messages: Vec<RawMessage> = senders
        .iter()
        .enumerate()
        .map(|(index, sender)| {
            message(
                &format!("m{index}"),
                &format!("sender-{sender}"),
                &format!("합성{sender}"),
                0,
                "같은 초에 보낸 말",
            )
        })
        .collect();
    let (_dir, archive) = archive_with(&messages);

    let expected: Vec<Vec<String>> = (0..senders.len())
        .map(|index| vec![format!("m{index}")])
        .collect();
    assert_eq!(chunk_messages(&archive), expected, "chunks --chat order");

    let ids: Vec<String> = archive
        .chunks_for_chat("chat-synthetic")
        .expect("chunks")
        .into_iter()
        .map(|summary| summary.chunk_id)
        .collect();
    for (index, chunk_id) in ids.iter().enumerate() {
        let context = archive
            .chunk_context(chunk_id)
            .expect("chunk context")
            .expect("chunk exists");
        let previous = context.previous.map(|chunk| chunk.chunk_id);
        let next = context.next.map(|chunk| chunk.chunk_id);
        assert_eq!(
            previous,
            index.checked_sub(1).map(|before| ids[before].clone()),
            "previous of chunk {index}"
        );
        assert_eq!(next, ids.get(index + 1).cloned(), "next of chunk {index}");
    }
}
