# Contributing

Thanks for helping. lazykatok reads private conversations, so the rules below
are about keeping other people's data out of a public repository as much as
about code quality.

## Setup

- Apple Silicon Mac, Rust 1.91 (the version CI pins in `.github/workflows/ci.yml`
  and `rust-version` in `Cargo.toml`). With rustup: `rustup toolchain install 1.91.0`.
- Python 3.9 or newer for the release and privacy scripts (the macOS system
  `python3` is enough).
- Tests never need KakaoTalk installed. They run on synthetic fixtures in
  `tests/fixtures/` and synthetic SQLite databases built inside the tests.

## Checks every change must pass

CI runs all of these; run them locally on the pinned toolchain before opening a
pull request, because newer clippy releases accept code that 1.91 rejects:

```bash
cargo +1.91.0 fmt --all -- --check
cargo +1.91.0 clippy --all-targets -- -D warnings
KATOK_EMBEDDER=local-test cargo +1.91.0 test --all-targets
cargo +1.91.0 publish --dry-run
python3 scripts/verify_release_config.py
bash tests/verify_history_privacy.sh
python3 scripts/verify_history_privacy.py . origin/main..HEAD
```

`KATOK_EMBEDDER=local-test` keeps the test run offline; without it the semantic
tests download the local embedding model.

## Privacy rules

- Fixtures are synthetic. Never paste a real message, room name, nickname, user
  id, device UUID, database file name or home directory path into code, tests,
  docs, commit messages or issues, even one that would be easier than inventing
  a value. The privacy scripts reject the common shapes, but they cannot catch
  everything.
- When you check something against your own KakaoTalk install, report counts,
  ids and status rather than message content.
- Sending reaches real people and cannot be undone. Test sends only in your own
  chat with yourself.

## Pull requests

- One behavior per pull request, with a test that fails before the change and
  passes after it.
- Keep `README.md`, CLI `--help` text and privacy behavior in agreement in the
  same change.
- Describe what you ran to verify the change and what it printed.

See [AGENTS.md](AGENTS.md) for the architecture notes that coding agents
working on this repository follow.
