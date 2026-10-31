# lazykatok

로컬 우선(local-first) 카카오톡 터미널 클라이언트. 방 목록을 최신순으로 골라 들어가는
실시간 대화 TUI, 로컬 아카이브, 키워드·BM25·의미 검색을 하나의 CLI로 제공합니다.
Apple Silicon macOS 전용.

> **English.** lazykatok is a local-first KakaoTalk client for the macOS terminal:
> a room picker and live chat TUI with replies, a local SQLite archive, and keyword,
> BM25 and semantic search over it. It reads the KakaoTalk desktop app's own data on
> your Mac and sends nothing anywhere unless you configure an embedding endpoint.
> Install with `cargo install --git https://github.com/changeroa/lazykatok`, then run
> `lazykatok doctor --macos-probe`. The sections below list every command, setting
> and environment variable; [CONTRIBUTING.md](CONTRIBUTING.md) is in English.

[![CI](https://github.com/changeroa/lazykatok/actions/workflows/ci.yml/badge.svg)](https://github.com/changeroa/lazykatok/actions/workflows/ci.yml)

## 특징

- **대화 TUI (`lazykatok watch --select --reply`)** — 마지막 메시지가 최신인 방이 목록 맨 아래(입력
  프롬프트 바로 위)에 1번으로 오는 방 선택, 로컬 시각 `HH:MM` + 보낸 사람 열 + 연속
  메시지 `·` 묶음 + 날짜 구분선으로 정돈된 대화 뷰, readline식 입력 편집, 스크롤과
  새 메시지 표식. 폴링과 전송이 백그라운드에서 돌아 입력은 항상 반응형입니다.
- **답장 전송** — TUI에서 Enter로 카카오톡 앱에 실제 전송(macOS Accessibility 경로).
  전송 중에도 입력 가능하고, 동시 전송은 안전을 위해 하나로 직렬화됩니다.
- **로컬 검색** — 키워드 / BM25 / 임베딩 의미 검색. 대화 데이터는 전부 로컬
  SQLite 아카이브에 저장되고, 외부 임베딩 엔드포인트를 직접 지정하지 않는 한
  네트워크로 나가지 않습니다.

## 요구 사항

- Apple Silicon macOS
- 실행 중인 카카오톡 macOS 앱 (실시간 읽기·전송)
- Rust 1.91+ (직접 빌드 시)
- 터미널에 **전체 디스크 접근 권한**(카톡 DB 읽기)과 **접근성 권한**(전송) —

  `lazykatok permissions macos --accessibility`로 시스템 설정 > 개인정보 보호 및
  보안 화면을 열어 부여하고, 카톡 데이터 접근은 `lazykatok doctor --macos-probe`로
  확인하세요. 접근성 권한은 `doctor`가 확인하지 않습니다.

## 설치

```bash
cargo install --git https://github.com/changeroa/lazykatok
# 바이너리: ~/.cargo/bin/lazykatok (이 디렉터리가 PATH에 있어야 합니다)
```

클론해서 고쳐 쓰려면:

```bash
git clone https://github.com/changeroa/lazykatok.git
cd lazykatok
cargo install --path .     # 또는 cargo build --release → target/release/lazykatok
```

권한 설정을 차례로 안내받으려면 클론 안에서 `scripts/lazykatok-macos-setup.sh`를
실행하세요 ([docs/macos-first-run.md](docs/macos-first-run.md)).

## 빠른 시작

```bash
lazykatok doctor --macos-probe        # 카톡 앱·데이터 접근·설정 상태 점검
lazykatok sync --source macos         # 로컬 카톡 DB → 아카이브 동기화
lazykatok index                       # 의미 검색용 로컬 벡터 인덱스 생성
lazykatok watch --select --reply --accept-use-policy --source macos   # 방 선택 → 대화 TUI
lazykatok watch --select --reply --reply-no-open --accept-use-policy --source macos   # 이미 열린 방에만 전송
lazykatok search keyword 회식 --limit 5   # 키워드 검색 (기본 10건)
lazykatok search bm25 회식                # BM25 검색
lazykatok search semantic "언제 만나기로 했지"  # 의미 검색 (index 필요)
```

`--source`를 생략하면 설정 파일의 `source_adapter`(기본값 `fixture`, JSONL 경로
필요)를 쓰므로 실제 카톡을 읽을 때는 `--source macos`를 붙이세요. `--data-dir`과
`--config`, `--account`는 하위 명령 앞에 둡니다 (`lazykatok --data-dir <dir> sync ...`).

그 밖의 예:

```bash
lazykatok watch --select --reply --accept-use-policy --source macos
lazykatok search keyword <질의> --json
lazykatok search hybrid <질의> --json    # BM25 + 의미 검색을 RRF로 결합 (부모 윈도우 단위)
lazykatok transcript --chat <chat_id> --since 2026-08-01T00:00:00+09:00 --out exports/   # 그 시각 이후 메시지를 exports/ 아래 Markdown으로
```

## 계정 고르기

한 맥에서 카톡 계정을 두 개 이상 쓴 적이 있으면 로그아웃한 계정의 데이터베이스도
그대로 남습니다. 어떤 계정으로 읽을지는 `--account`로 고르고, 계정 목록은
`accounts`로 봅니다.

```bash
lazykatok accounts                      # 열리는 계정 목록 (최근 기록 순)
lazykatok accounts --json
lazykatok --account latest sync --source macos    # 카톡이 지금 쓰는 계정
lazykatok --account <user_id> sync --source macos # 목록에서 고른 계정
```

- `latest`는 데이터베이스가 가장 최근에 기록된 계정, 즉 카톡이 지금 쓰는 계정입니다.
  카톡 설정 파일에는 한 번이라도 로그인한 모든 계정의 기록이 남아 있어 그것만으로는
  현재 계정을 알 수 없습니다.
- 계정마다 아카이브가 따로 유지됩니다. 데이터 디렉터리에 이미 자리 잡은 계정은 그
  디렉터리를 그대로 쓰고, 다른 계정은 `<data-dir>/accounts/<user_id>`로 읽습니다.
  두 계정의 대화가 한 아카이브에 섞이지 않습니다.
- `watch --select`를 터미널에서 실행하면 계정이 둘 이상일 때 방 목록 전에 계정을
  먼저 묻습니다. 그냥 Enter를 누르면 1번(가장 최근 계정)이 선택됩니다.
- 목록에는 user id, 최근 기록 시각, 열리는 데이터베이스 수만 나옵니다. 이름이나
  파일명은 출력하지 않습니다.

## 설정

설정 파일은 선택입니다. 없으면 아래 기본값을 쓰고, 있으면 `--config <파일>`로
지정합니다 (자동으로 찾는 기본 경로는 없습니다). 모든 키와 기본값이 담긴 예시는
[examples/katok.toml](examples/katok.toml)이며, 모르는 키는 오류로 거부됩니다.

| 키 | 기본값 | 의미 |
| --- | --- | --- |
| `source_adapter` | `fixture` | `--source`를 생략했을 때 쓸 원본. `macos`로 바꾸면 매번 `--source macos`를 붙이지 않아도 됩니다. |
| `chunk_gap_group_seconds` | `600` | 단체방에서 이보다 긴 간격이 있으면 새 청크 |
| `chunk_gap_direct_seconds` | `1800` | 1:1 방에서 이보다 긴 간격이 있으면 새 청크 |
| `semantic_dir` | `semantic` | 벡터 인덱스 디렉터리 (상대 경로는 데이터 디렉터리 기준) |
| `embedder_provider` | `local` | 아래 임베딩 백엔드 참고 |
| `embedder_model` | `embeddinggemma-300m-q4` | 임베딩 모델 |
| `vector_dimension` | `768` | 벡터 차원 |
| `embedding_batch_size` | `64` | 한 번에 임베딩할 문단 수 |
| `snippet_length` | `80` | 검색 결과에 보일 본문 길이 |
| `embedder_url`, `embedder_api_key_env`, `query_prefix`, `passage_prefix` | 없음 | 아래 임베딩 백엔드 참고 |

그 밖의 전역 옵션과 환경 변수:

| 옵션 / 변수 | 의미 |
| --- | --- |
| `--data-dir <dir>` | 아카이브와 인덱스 위치. 기본값 `~/Library/Application Support/katok` |
| `--account <user_id\|latest>` | 읽을 카톡 계정 (아래 계정 고르기) |
| `KATOK_KAKAO_USER_ID` | 카톡 DB 복호화에 쓸 user id를 직접 지정 (자동 탐지가 틀릴 때) |
| `KATOK_FASTEMBED_CACHE_DIR` | 로컬 임베딩 모델 캐시 위치 |
| `KATOK_EMBED_*`, `KATOK_EMBEDDER` | 아래 임베딩 백엔드 표 |

## 임베딩 백엔드

의미 검색 인덱싱의 기본값은 로컬 `fastembed`(`embeddinggemma-300m-q4`, 768차원)
입니다. 외부 OpenAI 호환 임베딩 서버(OpenAI, llama.cpp, Ollama, vLLM, TEI 등)를
쓰려면 엔드포인트를 지정하세요. `KATOK_*`와 `LAZYKATOK_*` 접두사를 모두 인정합니다.

| 환경 변수 | 의미 |
| --- | --- |
| `KATOK_EMBED_URL` | 엔드포인트 URL. 설정하면 `openai-compatible` 백엔드가 선택됩니다. 단, `KATOK_EMBEDDER`나 설정 파일의 `embedder_provider`가 `mock`·`local-test`이면 네트워크를 쓰지 않는 그 모드가 유지됩니다. |
| `KATOK_EMBED_MODEL` | 요청에 보낼 모델 이름 (`openai/<model>` ID로 기록) |
| `KATOK_EMBED_DIM` | 벡터 차원 (예: `Qwen3-Embedding-0.6B`는 `1024`) |
| `KATOK_EMBED_API_KEY` | 보낼 `Authorization: Bearer` 토큰 |
| `KATOK_EMBEDDER` | 백엔드 선택: `local` · `mock` · `local-test` · `openai-compatible` · `http` |

URL은 `/embeddings`로 끝나면 그대로, `/v1`로 끝나면 `/embeddings`를, 그 외에는
`/v1/embeddings`를 붙여 호출합니다. 같은 설정을 `--config katok.toml`의
`embedder_url`(별칭 `embedder_endpoint`) · `embedder_model` · `vector_dimension` ·
`embedder_api_key_env` · `query_prefix` · `passage_prefix`로도 지정할 수 있습니다.
`passage_prefix`를 지정하면 ID에 접두사의 해시가 붙어(`openai/<model>+passage-prefix-<hash>`),
접두사를 바꾼 뒤의 `index`는 이전 벡터를 재사용하지 않고 다시 임베딩합니다.
로컬 모델 캐시는 `KATOK_FASTEMBED_CACHE_DIR`, 없으면 플랫폼 캐시 디렉터리의
`lazykatok/fastembed`를 사용합니다.

## TUI 사용법

방 목록은 마지막 메시지 시각이 오래된 방부터 위에서 아래로 정렬되고 번호는 아래가
1번입니다 — 가장 최근에 활동한 방이 입력 프롬프트 바로 위에 1번으로 놓입니다.
시각을 알 수 없는 방은 맨 위에 이름 순으로 표시됩니다.

| 키 | 동작 |
|---|---|
| `Enter` | 현재 초안을 선택한 방으로 전송 (`/send 메시지`도 동일) |
| `←` `→` `Home` `End`, `Ctrl-A/B/E/F` | 초안 커서 이동 |
| `Backspace` / `Delete`, `Ctrl-W/U/K` | 글자·단어·행 삭제 |
| `↑` `↓` / `PgUp` `PgDn` | 대화 스크롤 / 페이지 |
| `Esc` | 초안이 있으면 지우고, 비어 있으면 종료 |
| `Ctrl-C` 또는 `Ctrl-D` | 종료 |
| `/help`, `/quit` | 도움말, 종료 |

- 메시지는 `HH:MM 보낸사람 본문` 형태로, 같은 사람이 5분 안에 보낸 연속 메시지는
  `·`로 묶여 표시됩니다. 날짜가 바뀌면 날짜 구분선이 나타납니다. 색상 구분 없이
  기본 전경색으로 렌더링됩니다.
- 새 메시지가 도착해도 작성 중인 초안과 커서는 그대로 유지됩니다. 과거 기록을
  보는 중이면 화면은 흔들리지 않고 `N new messages` 표식만 표시됩니다.
- 전송은 즉시 `전송 중…` 상태 줄과 함께 백그라운드로 실행되고, 결과는
  `sent reply (N chars)` 또는 `send failed: ...` 줄로 알림됩니다. 전송 중 추가
  Enter는 초안을 지키고 이전 전송 완료를 기다립니다. 알 수 없는 슬래시 명령도
  전송하지 않고 초안을 그대로 남겨 둡니다.
- paste는 텍스트 삽입일 뿐 절대 전송을 유발하지 않습니다. 붙여넣은 줄바꿈과
  탭은 공백으로 변환됩니다.

### 전송 주의사항

전송은 되돌릴 수 없고 상대에게 실제로 도착합니다. `--reply-no-open`은
카톡 창을 띄우거나 포커스를 가져가지 않고 이미 열려 있는 방에만
조용히 전송합니다. 기본 경로는 필요할 때만 잠깐 커튼을 띄우고 카톡을 앞으로
가져옵니다. 자세한 정책은 [ACCEPTABLE_USE_POLICY.md](ACCEPTABLE_USE_POLICY.md)와
[DISCLAIMER.md](DISCLAIMER.md)를 참고하세요.

## 프라이버시

- 모든 대화 데이터는 로컬(`~/Library/Application Support/katok` — 이전 이름의
  디렉터리를 그대로 사용, `--data-dir`로 변경) 아카이브 `archive.sqlite3`에
  저장됩니다. 아카이브는 암호화되지 않은 SQLite 파일이며, 데이터 디렉터리는 소유자
  전용(0700) 권한으로 만들어집니다. `transcript`와 `media get`의 출력도 `--out`을
  주지 않으면 같은 디렉터리의 `transcripts/`, `media/` 아래에 쓰입니다.
- 텔레메트리가 없습니다. 의미 검색의 임베딩은 기본적으로 로컬 모델로
  계산됩니다. `KATOK_EMBED_URL`(또는 설정 파일의 `embedder_url`)로 OpenAI 호환
  엔드포인트를 직접 지정한 경우에만 인덱싱할 문단과 검색어가 그 엔드포인트로
  전송됩니다. 그 밖에 기본 로컬 백엔드의 첫 `index`는 모델 파일을 내려받고,
  `media get`과 `media backfill`은 첨부의 CDN 링크를 요청합니다
  (`media get --no-cdn`으로 끔).
- 이 저장소는 공개입니다. 실제 대화에서 유래한 어떤 값도 커밋되지 않으며,
  테스트는 합성 fixture만 사용합니다 (`scripts/verify_release_config.py`의
  프라이버시 오라클이 이를 계속 검사합니다).

## 문제 해결

| 증상 | 확인할 것 |
| --- | --- |
| `lazykatok: command not found` | `~/.cargo/bin`이 `PATH`에 있는지 (`export PATH="$HOME/.cargo/bin:$PATH"`) |
| `doctor --macos-probe`가 DB에 접근하지 못함 | 지금 쓰는 터미널 앱에 전체 디스크 접근 권한을 주고 터미널을 다시 시작 (`lazykatok permissions macos`) |
| `sync`가 JSONL 경로를 요구함 | `--source macos`를 붙이거나 설정 파일에 `source_adapter = "macos"` |
| 다른 계정의 대화가 보임 | `lazykatok accounts`로 목록을 보고 `--account latest` 또는 `--account <user_id>` |
| 전송이 `send failed`로 끝남 | 터미널 앱의 접근성 권한 (`lazykatok permissions macos --accessibility`), 카톡이 실행 중이고 로그인되어 있는지 |
| `search semantic` 결과가 없음 | 먼저 `lazykatok index`; 기본 로컬 백엔드는 첫 실행 때 모델을 내려받으므로 네트워크가 필요 |

## 개발

기여 방법과 모든 변경이 통과해야 하는 검사는 [CONTRIBUTING.md](CONTRIBUTING.md)에
있습니다.

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all-targets
cargo publish --dry-run
python3 scripts/verify_release_config.py
python3 scripts/verify_history_privacy.py . origin/main..HEAD
```

두 파이썬 스크립트는 macOS에 기본으로 들어 있는 파이썬(3.9)에서도 그대로 돌아갑니다.

## 라이선스

MIT. 이 프로젝트는 개인적으로만 사용 가능하고 사용 책임은 본인에게 있습니다. 타인에게 공유하거나 상업적 사용을 하지마세요.
