# MVP validation

Validated on 2026-09-30 with Rust 1.98.1, an Apple M4 Max, and macOS 26.6.2. Sources in `~/KataGo` and `~/katrain` were read-only references. Original SGFs and model weights remain outside Git.

## Behavioral test evidence

The initial implementation of each slice was a minimal interface that allowed its integration tests to run and fail on missing behavior before production implementation. The portable suite contains 21 integration tests:

| Slice | Observed red failure | Passing behavior |
| --- | --- | --- |
| SGF review and persistence | Import/play interfaces returned missing-behavior errors | Real parsing, legal captures/ko/pass, atomic illegal-move rejection, persistent nested branches, mainline layout, original preservation, and export/reimport |
| Analysis cache | Cache identity/storage interfaces returned missing-behavior errors | Reopen and rename reuse, partial coverage, valid zeros, profile/history separation, deeper results retained, cache eviction preserves reviews |
| Scheduler | Missing positions produced no request | Full shallow coverage before refinement, out-of-order replies, interactive priority/cancellation, stale replies ignored, noResults handled |
| Engine transport | Engine profile/spawn unavailable | Real subprocess pipes, partial/final results, held responses, termination acknowledgments, and EOF |
| Worker | Open command returned an empty document | Automatic analysis, stored settings, offline cache recovery, and autosaved variations through worker commands |
| UI | Board/navigation controls absent; keyboard input did nothing | Actual board/tree clicks and keyboard/pass input change and persist the selected position; original chart retains gaps and zeros |
| Desktop shell | Initial SGF never reached the desktop through the worker | Startup import and analysis, then actual UI clicks create a variation through worker/SQLite |
| Handicap identity | Reopening setup SGFs produced different semantic identities | Sorted setup properties stabilize identity across repeated parses and compressed setup coordinates |
| Variation after completed analysis | New branch never received refinement, causing a timeout | New variations are evaluated and refined even after the original game has finished |
| Tree visibility | First variation's hit rectangle overlapped the status area | Tree scroll area retains its allocated height; first branch is fully visible |
| Invalid evaluation | Worker remained running after receiving zero visits | Analysis stops with an error while review input remains usable |
| Midgame setup | Partial chart claimed every position had 64 visits | Setup edits remain on the board and final status reports incomplete coverage |
| App packaging | Stub script produced no bundle metadata | Real Mach-O executable, valid launch metadata, executable permissions, and verified local code signature |

Targeted counterfactual checks were restored after the expected failures:

- Removing variation autosave initially survived an insufficient test because a later selection saved the document. The test was strengthened to construct a new review immediately after each played variation. The same mutation then failed on a missing persisted node.
- Removing the SQL visit-count guard failed the cache test: one visit replaced a stored 64-visit result.
- Removing the scheduler's outstanding-coverage gate failed the scheduling test: another request appeared before missing coverage completed.
- Removing board-click dispatch failed the real-input UI test: the expected White stone remained absent.
- The tree visibility, invalid evaluation, incomplete-analysis status, handicap identity, and post-completion variation regressions were reproduced against the preceding implementation before their fixes.

Routine tests use response channels, explicit held-engine replies, and UI stepping; they do not assert GPU speed or sleep for guessed completion times. Graphics previews complement behavioral assertions and are not used as feature proof.

## Real engine and corpus

The explicit real-engine test passed against `/opt/homebrew/bin/katago`, version 1.18.2, with `b10c384h6nbttflrs.bin.gz`. It submits an actual multi-turn analysis request and verifies turn numbers, visit counts, finite score, and bounded winrate. The earlier engine probe also verified streaming and final completion after cancellation.

The release corpus check imported and replayed all **56 SGFs**, covering **10,797 played positions**, in **1.895 seconds** with no failures. This is import/replay time, not neural-network analysis time. No private game contents were copied into fixtures or documentation.

The native desktop binary and the packaged `.app`, launched through macOS Launch Services with `open`, both reported `Katastro renderer: Metal · Apple M4 Max`. The bundled smoke run loaded a private game staged under the ignored `local/` directory and populated all 231 chart positions in the normal application cache. A separate GPU render of the real desktop interaction test was visually inspected. macOS display capture was unavailable to this tool environment, so that preview is a rendered test frame rather than an OS screenshot.

## Application latency

`tests/performance.rs` measures the release worker through real import, scheduling, engine I/O, cache writes, and snapshot delivery. Each trial creates fresh databases and a new engine, evaluates a 25-move game, then a 210-move game on the same warm process. Refinement is paused after chart coverage. Reopening uses the read-only Open command to measure persisted-cache restoration independently of a new search.

| Trial | Engine state | Played positions including root | First estimate | Complete chart | Cached reopen |
| --- | --- | --- | --- | --- | --- |
| 1 | Cold | 26 | 4.394 s | 9.451 s | 1 ms |
| 1 | Warm | 211 | 170 ms | 2.310 s | 40 ms |
| 2 | Cold | 26 | 1.430 s | 2.531 s | 1 ms |
| 2 | Warm | 211 | 176 ms | 696 ms | 42 ms |
| 3 | Cold | 26 | 1.386 s | 3.475 s | 1 ms |
| 3 | Warm | 211 | 172 ms | 1.710 s | 50 ms |

These are local observations, not latency guarantees. The first process includes model/Metal initialization; GPU compilation caches and shared machine activity affect later trials. The engine-only baseline in `engine-probe.json` measured roughly 32.7 seconds for 64-visit analysis of a 220-move game. It used different games and cache controls, so it does not establish a precise application speedup. The application measurements demonstrate the useful outcome: full provisional coverage arrives before deeper work, and reopening restores cached coverage without waiting for KataGo.

## Reproduce

Run the standard commands and explicit asset-dependent commands in [README.md](../README.md). To generate an optional local desktop preview:

```sh
mkdir -p local
KATASTRO_UI_PREVIEW="$PWD/local/ui-preview.png" cargo test --test desktop
```

The test fixtures contain synthetic game data. Package validation runs automatically on macOS with desktop features enabled. The three real-engine/corpus/performance tests are ignored by default because their local asset prerequisites are explicit.
