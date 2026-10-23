use assert_cmd::Command;
use predicates::prelude::*;

fn fixture_path(name: &str) -> String {
    format!("{}/tests/fixtures/kakao/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn cli_doctor_reports_sync_and_index_freshness_when_search_needs_updates() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let data_dir = dir.path();
    let fixture = fixture_path("replies.jsonl");

    Command::cargo_bin("lazykatok")
        .expect("lazykatok binary")
        .env("HOME", dir.path())
        .args([
            "--data-dir",
            data_dir.to_str().expect("utf8 path"),
            "doctor",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"freshness\""))
        .stdout(predicate::str::contains("\"last_sync\": null"))
        .stdout(predicate::str::contains("\"status\": \"not_checked\""))
        .stdout(predicate::str::contains("\"sync_before_search\": true"));

    Command::cargo_bin("lazykatok")
        .expect("lazykatok binary")
        .args([
            "--data-dir",
            data_dir.to_str().expect("utf8 path"),
            "sync",
            "--source",
            "fixture",
            &fixture,
            "--json",
        ])
        .assert()
        .success();

    Command::cargo_bin("lazykatok")
        .expect("lazykatok binary")
        .env("KATOK_EMBEDDER", "local-test")
        .args([
            "--data-dir",
            data_dir.to_str().expect("utf8 path"),
            "index",
            "--json",
        ])
        .assert()
        .success();

    Command::cargo_bin("lazykatok")
        .expect("lazykatok binary")
        .env("HOME", dir.path())
        .args([
            "--data-dir",
            data_dir.to_str().expect("utf8 path"),
            "doctor",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"last_sync\""))
        .stdout(predicate::str::contains("\"source\": \"fixture\""))
        .stdout(predicate::str::contains("\"last_index\""))
        .stdout(predicate::str::contains(
            "\"embedder\": \"embeddinggemma/local-test\"",
        ))
        .stdout(predicate::str::contains("\"sync_before_search\": false"));
}

#[test]
fn cli_doctor_recommends_sync_when_the_archive_is_gone_before_any_index() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let data_dir = dir.path();
    let fixture = fixture_path("replies.jsonl");

    Command::cargo_bin("lazykatok")
        .expect("lazykatok binary")
        .args([
            "--data-dir",
            data_dir.to_str().expect("utf8 path"),
            "sync",
            "--source",
            "fixture",
            &fixture,
            "--json",
        ])
        .assert()
        .success();
    std::fs::remove_file(data_dir.join("archive.sqlite3")).expect("remove archive");

    let output = Command::cargo_bin("lazykatok")
        .expect("lazykatok binary")
        .env("HOME", dir.path())
        .args([
            "--data-dir",
            data_dir.to_str().expect("utf8 path"),
            "doctor",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let doctor: serde_json::Value = serde_json::from_slice(&output).expect("doctor json");
    let recommendation = &doctor["freshness"]["recommendation"];

    // `katok index` refuses to run without an archive, so recommending it here
    // sends the caller to a command that can only fail.
    assert_eq!(
        recommendation["sync_before_search"], true,
        "{recommendation}"
    );
    assert!(
        recommendation["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("archive is missing")),
        "{recommendation}"
    );
}
