//! `send --chat` turns a `chat_id` into the name KakaoTalk shows plus a rank among the rooms
//! sharing that name, so both have to describe the chat list as it is now.

use chrono::{TimeZone, Utc};
use lazykatok::{archive::Archive, types::RawMessage};

fn message(chat_id: &str, chat_name: &str, message_id: &str, minute: u32) -> RawMessage {
    RawMessage {
        account_hash: "acct-synthetic".to_string(),
        chat_id: chat_id.to_string(),
        chat_name: chat_name.to_string(),
        chat_type: "group".to_string(),
        message_id: message_id.to_string(),
        sender_id: "u1".to_string(),
        sender_nickname: "테스터".to_string(),
        timestamp: Utc
            .with_ymd_and_hms(2026, 1, 1, 9, minute, 0)
            .single()
            .expect("valid timestamp"),
        text: "합성 메시지".to_string(),
        message_type: "text".to_string(),
        reply_to_message_id: None,
    }
}

/// A renamed room keeps its old name on the rows the source no longer reports (KakaoTalk prunes
/// its own history, and the archive outlives that), so reading the name off any one archived
/// message can hand `send` a title that is no longer on the chat list.
#[test]
fn a_renamed_room_is_addressed_by_its_current_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let archive = Archive::open(&dir.path().join("archive.sqlite3")).expect("open archive");

    archive
        .sync_messages(&[message("chat-a", "옛 이름", "a1", 0)])
        .expect("first sync");
    // The room was renamed and `a1` has since been pruned from the source.
    archive
        .sync_messages(&[message("chat-a", "새 이름", "a2", 10)])
        .expect("second sync");

    let (name, rank) = archive
        .chat_identity("chat-a")
        .expect("look up chat")
        .expect("chat is archived");
    assert_eq!(name, "새 이름");
    assert_eq!(rank, 0);
}

/// A room renamed away from a name is no longer one of the rooms sharing it, even though its
/// older archived rows still carry that name.
#[test]
fn a_room_renamed_away_does_not_count_as_a_same_named_peer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let archive = Archive::open(&dir.path().join("archive.sqlite3")).expect("open archive");

    archive
        .sync_messages(&[
            message("chat-a", "합성 방", "a1", 10),
            message("chat-b", "합성 방", "b1", 20),
        ])
        .expect("first sync");
    archive
        .sync_messages(&[
            message("chat-a", "합성 방", "a1", 10),
            message("chat-b", "다른 방", "b2", 30),
        ])
        .expect("second sync");

    let (name, rank) = archive
        .chat_identity("chat-a")
        .expect("look up chat")
        .expect("chat is archived");
    assert_eq!(name, "합성 방");
    assert_eq!(rank, 0, "chat-b is no longer called 합성 방");
}

#[test]
fn same_named_rooms_are_ranked_most_recent_first() {
    let dir = tempfile::tempdir().expect("tempdir");
    let archive = Archive::open(&dir.path().join("archive.sqlite3")).expect("open archive");

    archive
        .sync_messages(&[
            message("chat-a", "합성 방", "a1", 10),
            message("chat-b", "합성 방", "b1", 20),
            message("chat-c", "합성 방", "c1", 30),
        ])
        .expect("sync");

    let rank = |chat_id: &str| {
        archive
            .chat_identity(chat_id)
            .expect("look up chat")
            .expect("chat is archived")
            .1
    };
    assert_eq!(rank("chat-c"), 0);
    assert_eq!(rank("chat-b"), 1);
    assert_eq!(rank("chat-a"), 2);
    assert!(archive
        .chat_identity("chat-missing")
        .expect("look up chat")
        .is_none());
}
