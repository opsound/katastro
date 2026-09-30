# MVP validation

Validated on 2026-09-30 with Rust 1.98.1, an Apple M4 Max, and macOS 26.6.2. Sources in `~/KataGo` and `~/katrain` were read-only references. Original SGFs and model weights remain outside Git.

## Behavioral test evidence

The initial implementation of each slice was a minimal interface that allowed its integration tests to run and fail on missing behavior before production implementation. The original MVP and file-dialog repair established 24 integration tests. The subsequent review improvements bring the routine suite to 38 tests; their evidence appears below:

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

The original `tests/performance.rs` measurements below used the previous policy that cached one-visit results. They are historical evidence, not measurements of the new deepest-only cache policy. The test now finishes a selected deep run before reopening and verifies the persisted subset, rather than expecting all cheap chart points on disk.

The benchmark measures the release worker through real import, scheduling, engine I/O, cache writes, and snapshot delivery. Each trial creates fresh databases and a new engine, evaluates a 25-move game, then a 210-move game on the same warm process. Refinement is paused after chart coverage. Reopening uses the read-only Open command to measure persisted-cache restoration independently of a new search.

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

## Native file-dialog crash regression

The original desktop tests imported their SGF through the startup argument; they did not invoke the operating-system picker. They therefore missed a crash in `rfd::FileDialog::pick_file`: its AppKit `runModal` reentered winit during an active render callback, which aborted with `tried to handle event while another event is currently being handled`.

All four file-picking paths now use `rfd::AsyncFileDialog` with the application's native window as parent. The UI retains a pending future and polls it without blocking. Its waker requests a repaint when selection or cancellation completes. Only one sheet can be outstanding. Open/Export selections become worker commands; engine/model selections become editable settings. Canceling leaves the current review intact.

Three integration tests use actual toolbar/settings clicks and controlled delayed dialog completions to verify SGF import, cancellation, continued worker/UI progress, duplicate-sheet prevention, variation export without changing the original, and persisted engine settings. The initial deferred Open test failed because its completion was discarded; retaining and polling the future made it pass.

An additional explicit native test runs a real eframe/AppKit window in a subprocess, opens and cancels Open twice and Export once, then reopens the saved review to verify preservation. It operates only on its own AppKit windows and needs no system accessibility permission. File-selection delivery is tested through controlled dialog results; the native test verifies real sheet presentation/cancellation and process survival. Restoring the synchronous native picker makes the test fail: the render callback blocks in the modal panel and the process cannot complete its sheet/cancellation sequence. The test terminates that subprocess on timeout. The user's original crash log independently records the winit reentrancy panic. The async implementation was restored and the native check rerun before delivery.

Run `cargo test --test native_dialogs -- --ignored --nocapture` in a graphical macOS session. This fourth prerequisite-dependent test stays explicit so routine test runs do not display operating-system dialogs.

## Navigation, board guidance, and continuous refinement

The integration tests reproduced missing behavior before the fixes: a selected move 80 columns along a long SGF disappeared from the timeline; Down left selection unchanged; pending analysis moved the score heading upward by 28 pixels; refinement stopped at 64 visits; one-visit estimates and unfinished streamed runs were cached; and candidate, next-move, and player-color markers were absent. A rendering check also found that the new candidate list cut off the winrate chart at the default window height, and a test reproduced that before moving the list below both charts. The emblem and navigation arrows now use painted geometry, avoiding missing font glyphs.

The current checks exercise real review commands and temporary SQLite stores, actual UI clicks/keys and rendered shapes, and a controlled engine subprocess:

- Left/Right keeps the full selected highlight inside the timeline viewport; tree-button focus does not disable navigation. Up/Down selects the nearest variation at the same column, skips empty lanes, respects boundaries, and persists selection.
- Pending, provisional, and deeper selected evaluations keep chart bounds fixed, including when candidate results arrive. Both charts fit at the default 1200×860 window size. Scrollbar space and detail slots are reserved.
- Original-game and variation positions continue through 256, 1024, and 4096-visit targets in the scheduler test. The worker remains running and reports its current target; Pause retains that target for display. A held subprocess stream proves that unfinished deep estimates remain visible while being absent from the cache.
- Completed results below 64 visits are transient. Deeper eligible results replace shallower rows; late shallower results do not downgrade them. Startup removes obsolete cheap rows, preserves deep legacy JSON without candidate fields, and restores saved user variations. Pausing/resuming an open review preserves its in-memory estimates.
- Engine candidates follow KataGo's `order`, including skipped-I GTP coordinates, board corners, and passing. The top five survive a deep cache round trip. One-visit requests include policy, and legal policy previews contain no invented per-move score or winrate.
- The best suggestion is visibly blue; board and suggestion-button clicks create/select the expected persistent variation, and parent suggestions disappear on a child with no analysis. Next-move preview consists of separate unfilled arcs in Black/White, follows the original first continuation even after adding a branch, and disappears at a leaf.
- Colored stones beside named players, the right-column depth label, the painted emblem, and visible painted navigation arrows are asserted from actual UI geometry and state changes.

Sixteen targeted mutations each produced a behavioral test failure before restoration: disabling timeline follow, collapsing the pending detail slot, suppressing vertical navigation, capping budgets at 64, allowing cheap caching, removing the deepest-result SQL guard, caching partial replies, reversing candidate order, disabling policy output, changing blue to orange, removing the dotted ring, making legend arrows invisible, giving Black a White stone, clearing the worker target, placing candidates above charts, and discarding transient values on resume. The player-color mutation initially survived because a transparent circle outline satisfied the Black-fill assertion; requiring an opaque fill made that same mutation fail. Mutation output is kept in ignored local logs; every implementation was restored before final checks.

The real-engine test uses a synthetic 9×9 game and the local Metal KataGo/model, runs all three positions through 1, 8, 64, and 256 visits, verifies early policy suggestions, confirms five searched candidates at 256 visits, and reopens SQLite to compare the complete cached evaluations and candidate lists. It complements the subprocess fixture with actual protocol compatibility. Rendered board guidance was also inspected using `KATASTRO_BOARD_PREVIEW="$PWD/local/board-markers.png" cargo test --test ui suggestions_are_ranked`.

### Current latency check

The updated release benchmark passed three trials using the same private corpus and local engine/model. Each cache reopen restored exactly the two completed deep positions present in that trial, rather than the full provisional chart. All cached results had at least 64 actual visits.

| Trial | Engine state | Positions | First estimate | Full provisional chart | Deep-cache reopen |
| --- | --- | --- | --- | --- | --- |
| 1 | Cold | 26 | 1.442 s | 2.556 s | 1 ms |
| 1 | Warm | 211 | 1.212 s | 1.809 s | 41 ms |
| 2 | Cold | 26 | 1.424 s | 3.552 s | 1 ms |
| 2 | Warm | 211 | 180 ms | 724 ms | 41 ms |
| 3 | Cold | 26 | 1.473 s | 2.620 s | 1 ms |
| 3 | Warm | 211 | 1.215 s | 3.854 s | 40 ms |

These measurements include the new policy previews and deepest-only cache semantics. They demonstrate early provisional coverage and reuse of completed deep results; they do not establish a speedup over the historical baseline, whose cache policy differed. Final standard formatting, Clippy with warnings denied, and all 38 routine integration tests passed. The explicit real-engine tests passed (including 256-visit refinement and candidate round trips), and the native Open/Export sheet regression passed in 4.40 seconds.

## Comparable point deltas on the board

The user selected signed score change from the current position, from the moving player's perspective. Five additional integration tests bring the routine suite to 43 tests. Their initial failures showed different circle radii, rank numbers instead of point changes, and no label on the recorded move.

AI candidates and the next recorded move now use one radius and shared numeric formatting. A Black move computes `resulting Black score - current Black score`; a White move reverses that sign. Missing estimates show `--`, and rounded zero displays `+0.0`. The best candidate remains blue. When that candidate was also played, the dotted ring identifies it and one label uses the same search estimate, avoiding a contradictory value from an older child search. A played move outside the candidate list uses the child-position evaluation. Labels update as analysis refines and derive from existing cached results when reopening offline.

Tests exercise rendered text and geometry through real SGFs, UI input, and temporary SQLite stores. They cover Black and White signs, gains and losses, equal circle sizes, policy previews without fabricated scores, a played candidate with deliberately conflicting old child analysis, selecting that existing move without duplicate branches, refinement and offline cache restoration, and labels fitting a 19×19 board at the minimum window size. Seven temporary regressions produced behavioral failures: smaller candidate circles, omitting the current-score baseline, reversing White's sign, hiding the recorded label, drawing overlapping duplicate labels, turning missing estimates into zero, and disabling font fitting. Each was restored before final checks.

Formatting, Clippy with warnings denied, and all 43 routine integration tests passed. The native Open/Export sheet check passed in 4.13 seconds. The engine protocol and cache schema did not change for this UI improvement.
