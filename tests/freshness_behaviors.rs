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

/// The doctor payload is where "which files is this install reading" gets answered, next to
/// `data_dir` and `semantic_index`. The archive entry must carry its path, not only a status.
#[test]
fn cli_doctor_reports_the_archive_path_beside_its_status() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let data_dir = dir.path().to_str().expect("utf8 path").to_string();
    let doctor = || -> serde_json::Value {
        let output = Command::cargo_bin("lazykatok")
            .expect("lazykatok binary")
            .env("HOME", dir.path())
            .args(["--data-dir", &data_dir, "doctor", "--json"])
            .output()
            .expect("run doctor");
        serde_json::from_slice(&output.stdout).expect("doctor json")
    };
    let expected_path = format!("{data_dir}/archive.sqlite3");

    let before = doctor();
    assert_eq!(
        before["archive"]["path"].as_str(),
        Some(expected_path.as_str()),
        "{before}"
    );
    assert_eq!(before["archive"]["status"], "missing", "{before}");

    Command::cargo_bin("lazykatok")
        .expect("lazykatok binary")
        .args([
            "--data-dir",
            &data_dir,
            "sync",
            "--source",
            "fixture",
            &fixture_path("replies.jsonl"),
            "--json",
        ])
        .assert()
        .success();

    let after = doctor();
    assert_eq!(
        after["archive"]["path"].as_str(),
        Some(expected_path.as_str()),
        "{after}"
    );
    assert_eq!(after["archive"]["status"], "present", "{after}");
}

/// `doctor` must not call an index fresh that semantic search will refuse. An index built at one
/// embedding dimension is unusable once the configured dimension changes.
#[test]
fn cli_doctor_asks_for_an_index_when_the_embedding_dimension_changed() {
    let dir = tempfile::tempdir().expect("create tempdir");
    let data_dir = dir.path().to_str().expect("utf8 path").to_string();
    let lazykatok = |dimension: &str, args: &[&str]| {
        Command::cargo_bin("lazykatok")
            .expect("lazykatok binary")
            .env("HOME", dir.path())
            .env("KATOK_EMBEDDER", "local-test")
            .env("KATOK_EMBED_DIM", dimension)
            .arg("--data-dir")
            .arg(&data_dir)
            .args(args)
            .output()
            .expect("run lazykatok")
    };
    let fixture = fixture_path("replies.jsonl");
    assert!(
        lazykatok("768", &["sync", "--source", "fixture", &fixture, "--json"])
            .status
            .success()
    );
    assert!(lazykatok("768", &["index", "--json"]).status.success());
    let doctor = |dimension: &str| -> serde_json::Value {
        serde_json::from_slice(&lazykatok(dimension, &["doctor", "--json"]).stdout)
            .expect("doctor json")
    };
    assert_eq!(
        doctor("768")["freshness"]["recommendation"]["index_before_semantic_search"],
        false
    );

    let search = lazykatok("384", &["search", "semantic", "synthetic", "--json"]);
    assert!(!search.status.success(), "search must refuse the old index");
    let doctor = doctor("384");
    assert_eq!(
        doctor["freshness"]["recommendation"]["index_before_semantic_search"], true,
        "{doctor}"
    );
}
