use assert_cmd::Command;
use std::path::Path;

const DELETED: &str = "synthetic secret to be pruned";

fn row(id: &str, timestamp: &str, text: &str) -> String {
    format!(
        "{{\"account_hash\":\"acct-x\",\"chat_id\":\"chat-prune\",\"chat_name\":\"Synthetic\",\
         \"chat_type\":\"group\",\"message_id\":\"{id}\",\"sender_id\":\"u1\",\
         \"sender_nickname\":\"tester\",\"timestamp\":\"{timestamp}\",\"text\":\"{text}\",\
         \"message_type\":\"text\",\"reply_to_message_id\":null}}\n"
    )
}

fn lazykatok(data_dir: &Path, args: &[&str]) -> serde_json::Value {
    let output = Command::cargo_bin("lazykatok")
        .expect("lazykatok binary")
        .env("KATOK_EMBEDDER", "local-test")
        .arg("--data-dir")
        .arg(data_dir)
        .args(args)
        .output()
        .expect("run lazykatok");
    assert!(
        output.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).expect("json output")
}

fn files_holding(root: &Path, needle: &[u8]) -> Vec<String> {
    let mut hits = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if std::fs::read(&path)
                .expect("read file")
                .windows(needle.len())
                .any(|window| window == needle)
            {
                hits.push(
                    path.strip_prefix(root)
                        .expect("relative")
                        .display()
                        .to_string(),
                );
            }
        }
    }
    hits
}

/// Syncs three messages and indexes them, then writes a fixture without the middle one.
fn archive_with_an_indexed_message_to_prune(dir: &Path) -> (std::path::PathBuf, String, String) {
    let data_dir = dir.join("data");
    let full = dir.join("full.jsonl");
    let pruned = dir.join("pruned.jsonl");
    std::fs::write(
        &full,
        [
            row("m0", "2026-01-01T00:00:00Z", "first synthetic line"),
            row("m1", "2026-01-01T00:00:10Z", DELETED),
            row("m2", "2026-01-01T00:00:20Z", "last synthetic line"),
        ]
        .concat(),
    )
    .expect("write fixture");
    std::fs::write(
        &pruned,
        [
            row("m0", "2026-01-01T00:00:00Z", "first synthetic line"),
            row("m2", "2026-01-01T00:00:20Z", "last synthetic line"),
        ]
        .concat(),
    )
    .expect("write pruned fixture");
    let full = full.to_str().expect("utf8").to_string();
    lazykatok(&data_dir, &["sync", "--source", "fixture", &full, "--json"]);
    lazykatok(&data_dir, &["index", "--json"]);
    assert!(
        !files_holding(&data_dir, DELETED.as_bytes()).is_empty(),
        "the setup must leave the text on disk before the prune"
    );
    (data_dir, full, pruned.to_str().expect("utf8").to_string())
}

#[test]
fn prune_deleted_leaves_no_copy_of_the_removed_text_under_the_data_dir() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (data_dir, _full, pruned) = archive_with_an_indexed_message_to_prune(dir.path());

    let report = lazykatok(
        &data_dir,
        &[
            "sync",
            "--source",
            "fixture",
            &pruned,
            "--prune-deleted",
            "--json",
        ],
    );

    assert_eq!(report["pruned_messages"].as_array().map(Vec::len), Some(1));
    assert_eq!(report["semantic_index_removed"], serde_json::json!(true));
    assert_eq!(
        files_holding(&data_dir, DELETED.as_bytes()),
        Vec::<String>::new(),
        "files still holding the pruned text"
    );
}

#[test]
fn prune_preview_and_a_prune_that_removes_nothing_keep_the_semantic_index() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (data_dir, full, pruned) = archive_with_an_indexed_message_to_prune(dir.path());

    let preview = lazykatok(
        &data_dir,
        &[
            "sync",
            "--source",
            "fixture",
            &pruned,
            "--prune-preview",
            "--json",
        ],
    );
    assert_eq!(preview["semantic_index_removed"], serde_json::json!(false));
    let nothing = lazykatok(
        &data_dir,
        &[
            "sync",
            "--source",
            "fixture",
            &full,
            "--prune-deleted",
            "--json",
        ],
    );
    assert_eq!(nothing["semantic_index_removed"], serde_json::json!(false));

    assert!(data_dir.join("semantic").exists());
    let search = lazykatok(&data_dir, &["search", "semantic", "synthetic", "--json"]);
    assert!(search.as_array().is_some_and(|hits| !hits.is_empty()));
}
