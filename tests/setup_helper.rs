//! The macOS setup helper must find a binary built in its own clone, from any working directory.
//! A stub binary stands in for lazykatok, so no permission pane or KakaoTalk is ever touched.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fake_clone(builds: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let clone = dir.path().join("clone");
    std::fs::create_dir_all(clone.join("scripts")).expect("scripts dir");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/lazykatok-macos-setup.sh"),
        clone.join("scripts/lazykatok-macos-setup.sh"),
    )
    .expect("copy helper");
    for build in builds {
        let bin_dir = clone.join("target").join(build);
        std::fs::create_dir_all(&bin_dir).expect("bin dir");
        let stub = bin_dir.join("lazykatok");
        std::fs::write(
            &stub,
            format!("#!/bin/sh\necho \"{build} $*\" >> \"$STUB_LOG\"\nexit 0\n"),
        )
        .expect("write stub");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))
                .expect("chmod stub");
        }
    }
    (dir, clone)
}

/// Runs the helper from `cwd` with no lazykatok on PATH and closed stdin, so it stops at the
/// first prompt. Returns the stub's log.
fn run_helper(clone: &Path, cwd: &Path, log: &Path) -> String {
    Command::new("bash")
        .arg(clone.join("scripts/lazykatok-macos-setup.sh"))
        .current_dir(cwd)
        .env("PATH", "/usr/bin:/bin")
        .env_remove("LAZYKATOK_BIN")
        .env("STUB_LOG", log)
        .stdin(std::process::Stdio::null())
        .output()
        .expect("run helper");
    std::fs::read_to_string(log).unwrap_or_default()
}

#[test]
fn helper_finds_the_clone_build_when_run_from_another_directory() {
    let (dir, clone) = fake_clone(&["debug"]);
    let elsewhere = dir.path().join("elsewhere");
    std::fs::create_dir_all(&elsewhere).expect("elsewhere");
    let log = dir.path().join("stub.log");

    let calls = run_helper(&clone, &elsewhere, &log);

    assert!(
        calls.contains("debug permissions macos --accessibility"),
        "the helper did not run the clone's build: {calls:?}"
    );
}

#[test]
fn helper_prefers_a_release_build_over_a_debug_build() {
    let (dir, clone) = fake_clone(&["debug", "release"]);
    let log = dir.path().join("stub.log");

    let calls = run_helper(&clone, &clone, &log);

    assert!(
        calls.starts_with("release permissions macos"),
        "the helper did not run the release build: {calls:?}"
    );
}
