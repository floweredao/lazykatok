---
name: katok
description: Search local KakaoTalk keyword, BM25, and EmbeddingGemma vector indexes through the lazykatok CLI, list rooms, export chunks or transcripts, extract attachments, and prepare guarded message sends only after explicit user confirmation.
---

# katok

Use the `lazykatok` CLI as the only execution surface. This skill stays thin: it checks readiness, calls CLI search commands, retrieves explicit chunks when needed, and summarizes results.

## Privacy Rules

- Do not inspect local database internals from the skill.
- Do not handle auth caches or decryption material.
- Do not read KakaoTalk DB files directly. Use `lazykatok sync --source macos --json`.
- Search commands return minimal snippets and chunk ids by default.
- Full chunk content is shown only when the user explicitly asks for an exact result, asks to open a result, or provides a chunk id.

## Commands

```bash
export PATH="$HOME/.cargo/bin:$PATH"       # use when lazykatok is not found after cargo install
lazykatok doctor --json
lazykatok accounts --json                     # KakaoTalk accounts whose databases open here
lazykatok --account <user-id|latest> sync --source macos --json   # read one account; each keeps its own archive
lazykatok permissions macos                   # opens Full Disk Access settings
lazykatok permissions macos --accessibility   # also opens Accessibility settings
lazykatok doctor --macos-probe --json        # explicit macOS permission/app-data probe
lazykatok sync --source macos --json          # reads live macOS KakaoTalk (needs Full Disk Access)
lazykatok sync --json                         # uses source_adapter from config
lazykatok index --json                        # builds local EmbeddingGemma vector index
lazykatok index --full --json                 # full rebuild instead of incremental
lazykatok wipe-index --yes                    # drops the local index
lazykatok source chats --source macos --json  # lists rooms with their chat ids
lazykatok search keyword "검색어" --json
lazykatok search bm25 "검색어" --json
lazykatok search semantic "지난 회의 보고서" --json
lazykatok search hybrid "지난 회의 보고서" --json   # BM25 + semantic, fused over parent windows
lazykatok chunk get <chunk-id> --json
lazykatok chunk get <chunk-id> --redact --json
lazykatok chunk context <chunk-id> --json
lazykatok chunk parent <chunk-id> --json
lazykatok chunks --chat <chat-id> --json      # every chunk in one room (metadata only)
lazykatok transcript --chat <chat-id> --json                      # exports one room to Markdown
lazykatok transcript --chat <chat-id> --since 2026-07-20 --json   # only messages at or after a time
lazykatok transcript --chat <chat-id> --out <dir> --json          # writes outside the data dir
lazykatok media get --chat <chat-id> --out <dir> --json   # photos, albums, videos, files, and emoticons
lazykatok media get --chat <chat-id> --kind file --json   # only file attachments (zip, pdf, xlsx, ...)
lazykatok media get --chat <chat-id> --out <dir> --no-cdn --json   # local tiers only
lazykatok media backfill --dry-run --json   # what is still fetchable across every room
lazykatok media backfill --json             # save it before the presigned links expire
lazykatok send --chat <chat-id> --dry-run --json
```

For synthetic QA only:

```bash
lazykatok --data-dir <tmp> sync --source fixture tests/fixtures/kakao/replies.jsonl --json
KATOK_EMBEDDER=local-test lazykatok --data-dir <tmp> index --json
KATOK_EMBEDDER=mock lazykatok --data-dir <tmp> index --json
```

Synthetic runs must be isolated with the `--data-dir <tmp>` flag. It is a top-level flag, so it goes
before the subcommand; `lazykatok sync --data-dir <tmp>` is rejected. There is no `KATOK_DATA_DIR`
environment variable; setting one is silently ignored and the run writes into the real archive.

## Operating Pattern

1. If `lazykatok` is not found after install, run `export PATH="$HOME/.cargo/bin:$PATH"` and retry.
2. If macOS permission setup is needed, run `lazykatok permissions macos` so the user can grant Full Disk Access in System Settings.
3. Run `lazykatok doctor --json` before search to inspect freshness without triggering macOS app-data permission prompts.
4. Inspect the `freshness` section from `doctor --json` before search.
5. Run `lazykatok sync --source macos --json` when `freshness.recommendation.sync_before_search` is `true`, when the user asks for recent messages, or when search freshness matters.
6. Run `lazykatok index --json` before semantic search when `freshness.recommendation.index_before_semantic_search` is `true` or after a sync that should affect vector search.
7. Use `lazykatok search keyword ...`, `lazykatok search bm25 ...`, `lazykatok search semantic ...`, and `lazykatok search hybrid ...` for discovery.
   - A search returns a global top-k across rooms (default 10). For a person or topic that may occur across many rooms, raise `--limit` (for example 100) and group the returned hits by the stable `chat_id`; a room outside the first 10 is not an engine omission.
8. Use `lazykatok chunk get ...` only for explicit retrieval.
9. Run `lazykatok doctor --macos-probe --json` only for setup or permission diagnostics, because it may trigger a macOS "access data from other apps" prompt.

## Message Sending Safety

Sending is an irreversible external side effect. Use the dedicated
[`katok-send`](../katok-send/SKILL.md) skill rules whenever the user explicitly
asks to send or stage a message.

- Never infer send permission from a search, summary, drafting, or setup request.
- Never pass `--accept-use-policy` until the user has explicitly confirmed the
  exact room and final message or image in the current interaction.
- Run `lazykatok send --chat <chat-id> --dry-run --json` first.
- Prefer `--chat` over a room name; names are not unique.
- Do not automate loops, schedules, recipient lists, bulk delivery, or retries.
- Do not assist spam, impersonation, account theft, post-refusal contact,
  stalking, harassment, privacy violations, or protection-measure evasion.
- Read `ACCEPTABLE_USE_POLICY.md` and `DISCLAIMER.md`; Accessibility permission
  is not Kakao approval.
- Image paste UI failures (webp confirm miss → PNG once, etc.): see
  [`katok-send` Troubleshooting](../katok-send/SKILL.md#troubleshooting).

`--source macos` reads the live macOS KakaoTalk SQLCipher database locally in Rust; the terminal must have Full Disk Access to `~/Library/Containers/com.kakao.KakaoTalkMac/`. The store can also live in `~/Library/Application Support/com.kakao.KakaoTalkMac/`; both roots are scanned and the most recently modified copy of each database wins.

Use `lazykatok chunk context <chunk-id> --json` to inspect the immediate previous and next micro chunks in the same chat. Use `lazykatok chunk parent <chunk-id> --json` to jump from a micro chunk to its larger 5-minute same-chat window parent chunk. Semantic and hybrid search return parent-window hits with `child_chunk_ids`; use these chunk commands to navigate from broad context back to exact messages.

`lazykatok search hybrid` fuses the BM25 and semantic rankings with reciprocal rank fusion (k=60). BM25 ranks micro chunks and semantic search ranks parent windows, so each BM25 chunk is counted as a hit on the window(s) containing it before fusion; a window holding several matching chunks counts once, at its best chunk's rank. Each input contributes `3 × --limit` candidates. Hits are parent windows (`unit: parent_window`, `ranker: hybrid`), like semantic search. It needs a current semantic index and fails with the semantic search error (`semantic_index_missing`, `semantic_index_stale`, ...) rather than falling back to keyword-only hits.

`lazykatok index` runs the local `embeddinggemma-300m-q4` embedder in-process by default. Do not ask the user to start a Python, Jina, TEI, or local HTTP embedding server. Use `KATOK_EMBEDDER=mock` only for synthetic QA and `KATOK_EMBEDDER=local-test` only when you need deterministic local vector tests without downloading the model.

The index never follows the KakaoTalk database on its own, so a search only ever reflects the last sync. Run `lazykatok sync --source macos --json` before the first query of a session and before any question about recent messages. After sync, doctor compares the archive revision with the committed semantic generation and requests indexing when they differ; semantic search also refuses stale or orphaned generations instead of silently searching them. Freshness also depends on KakaoTalk itself running (`pgrep -x KakaoTalk`), because the source database receives new messages only while the app is up.

Sync is cheap enough to run often because only chats whose messages changed have their chunk tails recomputed. Three runs still pay the full cost: the first sync on an empty archive, the first sync after `chunk_gap_group_seconds` or `chunk_gap_direct_seconds` changes, and the first sync after an upgrade that bumps the chunker version, which includes the first run against an archive written before the version was recorded. Each of these invalidates every stored chunk once, after which sync returns to the incremental path. The payload reports `rebuilt_chats` and a `timings_ms` breakdown (`read_source`, `upsert_messages`, `rebuild_chunks`), so a slow run can be attributed to a stage instead of guessed at.

## Field Notes

- Most group rooms come back as a `chat-<id>` placeholder rather than a real title, because the source database does not always carry the room name. Grepping room names will not find them.
- To locate a room whose title is a placeholder, search for a term used inside it and group the hits by `chat_name`. The `chat_id` holding the most hits is that room. Each hit carries `chunk_id`, `chat_name`, `sender_nickname`, `started_at`, and `snippet`.
- `lazykatok chunks --chat <chat-id>` returns chunk metadata only, never `text`. To export a whole room, use `lazykatok transcript --chat <chat-id>`, or collect the chunk ids and run `lazykatok chunk get` per chunk, then assemble in timestamp order.
- `lazykatok chunk get --redact` masks the entire `text`, not only the PII inside it. Use it for a line you must quote, not for a readable export.
- KakaoTalk system feed entries (invite, join, leave) arrive as JSON strings such as `{"inviter":...}` or `{"member":...,"feedType":N}`. Filter them out of anything a person will read.

## Deleted Messages

Sync only upserts, so a message removed upstream stays in the archive unless it
is reconciled away:

```
lazykatok sync --source macos --prune-preview --json   # what the source no longer has; deletes nothing
lazykatok sync --source macos --prune-deleted --json   # actually remove it
```

Only the time range the source still reports for a chat is considered. KakaoTalk
prunes its own database and outliving that is the reason this archive exists, so
messages older than the source's reach are kept, and a chat the source did not
mention at all is skipped. Preview before deleting — lazykatok cannot undo it.

## Media Attachments

`lazykatok media get --chat <chat-id> --out <dir>` resolves each media message through four tiers in order: the decrypted local cache, a GET of the attachment's own presigned CDN URL verified by SHA-1, the decrypted `.thm` thumbnail, and finally a metadata stub. Apart from an OpenAI-compatible embedding endpoint the user configured and the local model download on the first `lazykatok index`, that CDN GET is the only network access in the CLI, and `--no-cdn` disables it so resolution stays entirely local.

Photos (type 2), albums (type 27), videos (type 3), and generic file attachments (type 18) are all covered, as are emoticons and stickers (types 12, 20, 25); `--kind photo|video|file|emoticon` narrows a run and defaults to every kind. Photos and albums read the `.img` cache; videos read the `.vid` cache, whose filename stem uses a `v` key prefix instead of `p`. For those, the output extension is sniffed from the decoded body, so a video lands as `.mp4`. An emoticon has no local tier and no `cs`: its image is fetched from the public catalog host (`https://item.kakaocdn.net/dw/<path>`) by the attachment's `path`, and stored only when the body sniffs as an image.

**A file attachment is one message type covering every extension** — zip, pdf, xlsx, hwp, pptx, csv, mp3 and the rest — so nothing needs adding per format.

Two things make files behave unlike photos:

- **They have no local cache at all.** KakaoTalk writes only `.thm`, `.img`, and `.vid` into the container; a file attachment never touches disk. The CDN is its only tier, `--no-cdn` returns nothing for it, and a stub for a file says `unavailable` rather than `not-cached` because there was never a cache that could have held it.
- **The attachment `name` is authoritative for the extension**, not the body. A zip sniffs as `.bin`, so output is written as `<logId>_<original name>` with the name sanitised so it cannot escape the output directory.

Three practical notes for video:

- The local `.vid` cache only holds videos that were actually played on this Mac. Anything else must come from the CDN tier, so `--no-cdn` will miss it.
- The presigned CDN URL carries an `expires` epoch. Past that, the video is unrecoverable from the archive and only the sender can re-send it.
- KakaoTalk re-encodes video on send. The retrieved file is the compressed copy the room received, not the sender's original.

## Preserving Attachments Before They Expire

A presigned URL lasts roughly 14 days. For a video that means falling back to whatever is cached; for a file, which has no cache, expiry is permanent loss. `katok media backfill` exists for that window:

```
katok media backfill --dry-run --json    # what would be fetched, no requests at all
katok media backfill --json              # every room, live links only, kind=file by default
```

It walks every room holding media, skips anything already saved without a network call — so re-running is idempotent and resumes an interrupted run — and reports per-tier totals. `--dry-run` distinguishes `planned` (would fetch) from `stub`/`cdn-expired` (already gone), which switching the CDN tier off cannot do. A dry run writes no file: a photo the local cache would serve is reported as `full`/`thumb` with no path.

Run it on a schedule if attachments matter. A backfill started after the window has closed cannot recover anything: report the expired count honestly rather than implying the files can be retrieved.

## Related Skills

`lazykatok send` is documented separately in `skills/katok-send/SKILL.md`. It exists
only in macOS builds with the default `private-send` feature. It is the
only subcommand that writes, and it does so by driving the running official
KakaoTalk app's UI rather than a Kakao remote private protocol or API.

## Platform

Assume Apple Silicon macOS. Intel macOS is not a supported target for the packaged local EmbeddingGemma path.
