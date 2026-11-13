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

The project requires integration tests before application behavior changes; see [AGENTS.md](../AGENTS.md) for the full workflow. Standard checks:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Real-engine, corpus, and performance checks require a local KataGo executable, a compatible model, and SGFs for the corpus and performance runs. Set these paths for your machine:

```sh
export KATASTRO_TEST_ENGINE=/opt/homebrew/bin/katago
export KATASTRO_TEST_MODEL="$HOME/katrain/katrain/models/b10c384h6nbttflrs.bin.gz"
export KATASTRO_SGF_DIR="$HOME/Downloads"
cargo test --release --test engine -- --ignored --nocapture
cargo test --release --test corpus -- --ignored --nocapture
cargo test --release --test performance -- --ignored --nocapture
```

To generate an optional local desktop preview, use a graphical macOS session with a working Metal adapter:

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

## Variation assessments in both charts

Five additional integration tests bring the routine suite to 48 tests. Before implementation, the original curve stayed bright past a fork, no variation assessments were drawn, and clicking a branch assessment selected the original position at that depth. All five tests ran and failed on those observable behaviors before the production change.

The charts now share a fixed Black perspective across the played game and variations. While a branch is selected, the common prefix retains its normal color, the original continuation becomes dim, and a gold curve follows the selected branch through its saved first-child continuation. Nested selection follows that branch's actual ancestors back to the first departure from the original game; it does not mix sibling assessments. The selected marker appears on its own evaluation, including when the position belongs to a variation. Clicking at a depth chooses the nearer of the active and original evaluations, with the active branch preferred for equal or unavailable assessments. Branches longer than the original game remain visible and selectable. Returning to the original game restores its full curve.

Tests use actual imported SGFs, legal variation moves, temporary SQLite reviews, real chart geometry, and pointer/tree input. They verify both points and winrate values, prefix/future styling, saved continuation beyond the selected node, nested and sibling selection, review reopen, disconnected analysis gaps, valid zero results arriving during refinement, the selected marker, and chart navigation including beyond the original game's final move. Eight temporary mutations failed before restoration: removing dimming, truncating saved continuation, dimming from a nested fork instead of the first departure, bridging missing evaluations, dropping zero assessments, restricting the selected marker to the original game, routing every chart click to the original game, and clipping variation data to original length.

The engine protocol, scheduler, and cache schema are unchanged. An optional rendered preview can be generated with `KATASTRO_CHART_PREVIEW="$PWD/local/variation-charts.png" cargo test --test ui variation_charts_dim -- --nocapture`.

Final formatting, Clippy with warnings denied, and all 48 routine integration tests passed. The explicit native Open/Export sheet check passed in 3.78 seconds. The rendered variation-chart preview was generated and visually inspected in addition to the behavioral assertions.

## All candidates with point-loss colors

The user chose every returned engine candidate with strength-based colors. Five new UI integration tests bring the routine suite to 53 tests; two existing scheduler/cache tests were strengthened to require candidates beyond five. Before implementation, those scheduler tests failed on truncated searched and policy lists. The five UI tests failed on missing later board candidates, identical green colors for different losses, unreachable sidebar candidates, and a poor Pass alternative lacking a strength color. Test harness scrolling uses simulated frames until animations settle, without wall-clock sleeps.

The parser now retains the complete ranked `moveInfos` list and all legal policy previews. Both the board and the sidebar use a shared color function: best searched recommendation blue; alternatives green through 0.5 points lost, then continuously interpolated through yellow at 1.5, orange at 3, and red at 6 or more. Loss uses the moving player's perspective and the best recommendation's score, rather than rank or the root score. Unscored moves are neutral gray; the leading policy preview has a blue outline. Alternatives below 25 visits use reduced opacity, with search effort distinguished from a formal confidence measure. Circle labels retain the user's previously chosen point change from the current position, including when that differs from the color metric.

The candidate sidebar virtualizes every row within a reserved height and retains wheel gestures at its boundaries, preventing list scrolling from displacing the charts. Every board candidate remains clickable through the board's normal input, and later sidebar candidates including Pass create/select persistent review nodes. The recorded move shares one label with a matching candidate even beyond rank five. The legend explains the color thresholds and its tooltip describes subdued and unknown evaluations.

Tests cover Black and White, near-best moves, intermediate and severe losses, smooth color changes under small refinements, low visit counts, missing scores becoming evaluated, a 2×2 position with only one empty intersection plus Pass, policy-preview styling, later board/sidebar input, persistence, candidate/recorded overlap beyond five, and stable charts during sidebar scrolling. Fifteen temporary regressions each produced behavioral failures before restoration: searched-parser cap, policy-parser cap, board cap, duplicate recorded labels beyond five, rank-only green colors, the wrong score baseline, reversed White perspective, full-strength low-visit colors, green unknown scores, quantized color steps, a green best recommendation, missing policy outline, sidebar cap, wheel leakage, and suppressed later-row dispatch.

The real Metal KataGo/model check refined three synthetic 9×9 positions through 1, 8, 64, and 256 visits. At each reply it compared the parsed candidate count with the actual engine output. Final searched counts were 25, 12, and 12; reopening SQLite restored the full evaluations and candidate arrays exactly. The cache schema and analysis compatibility profile are unchanged, preserving eligible older deep results; previously truncated candidate lists expand when analysis refines. An optional board preview uses `KATASTRO_CANDIDATE_PREVIEW="$PWD/local/candidate-colors.png" cargo test --test ui all_scored_candidates_are_visible -- --nocapture`.

Final formatting, Clippy with warnings denied, and all 53 routine integration tests passed. Both explicit real-engine tests passed in 6.58 seconds; the native Open/Export sheet regression passed in 3.76 seconds. The candidate-color preview was generated and visually inspected alongside the behavioral assertions.

## No full-board policy flash after playing

Showing every legal policy preview on the board caused a new regression: after a stone was placed, its one-visit evaluation briefly filled the remaining intersections with gray markers. Two additional integration tests bring the routine suite to 55 tests. Before implementation, the first test placed a real board move and accepted a scheduler response containing 361 legal policy previews on a 19×19 board; the first policy frame failed on a filled gray circle at A19. The second test failed because an unscored preview filled the next recorded move's hollow marker.

The board now draws only candidates with a point estimate. All policy data remains in memory and in the scrollable sidebar, including its last Pass entry; early position score/winrate still appear. There is no timer or visit-count threshold delaying searched candidates: a streamed eight-visit result immediately shows all seven scored candidates in the test, including a zero-visit candidate and candidates beyond five. Real board input on a later candidate creates a persistent variation. Current-position selection, rather than a retained parent overlay, determines the board annotations.

The recorded-move label suppresses duplicate text only when a matching candidate is actually drawn. A hidden policy preview therefore leaves the dotted marker unfilled and its child-based point delta visible, for both Black and White. Prior candidate tests now check gray unknown scores in sidebar buttons and equal marker sizes using scored candidates with an unevaluated recorded child.

Seven temporary regressions produced behavioral failures before restoration: drawing policy-only board markers, suppressing the recorded label because of a hidden preview, capping scored board candidates at five, hiding lightly searched candidates, truncating sidebar previews, discarding early position values, and retaining the parent's board suggestions. These tests use actual review/SQLite input, scheduler JSON acceptance, rendered geometry, and simulated UI frames; no wall-clock sleeps or GPU timing assertions are involved. The engine protocol, scheduler, and cache schema are unchanged.

Final formatting, Clippy with warnings denied, and all 55 routine integration tests passed. This repair changes board rendering and duplicate-label detection; the preceding real-engine and native-dialog evidence remains documented above.

## Group strength toggle

Five new behavioral integration tests bring the standard suite to 60. The minimal UI/analysis interface ran before implementation: the actual toggle produced no green tint at B8, missing ownership produced no pending indicator, refinement requests omitted ownership, and malformed arrays were accepted. The worker check also first failed with an engine fixture that omitted the requested ownership payload; that controlled subprocess now returns maps only when requested. These are behavior failures rather than compile failures.

The default-off toggle tints existing stones and outlines strict connected chains. Tests inspect actual painted circles and boundaries for Black and White, live/unsettled/dying readings, diagonal separation, connected-chain averaging, weak broken versus decisive solid outlines, and absence of internal seams. Clicking the toggle restores the normal stones. Board clicks still create autosaved variations, selection retains the toggle, parent ownership disappears on an unanalyzed child, and subsequent child estimates replace it. Legacy cached JSON lacking ownership stays pending instead of painting zero-valued groups; genuine zero ownership is unsettled. A 900×680 UI test verifies exact board bounds while ownership arrives and working Pass input.

Scheduler tests accept real protocol-shaped JSON through position/history mappings, retain ownership alongside score/winrate during 8/64/256 refinement, and reopen actual temporary SQLite stores to compare complete deepest results. An attempted 64-visit replacement cannot downgrade a cached 256-visit map. Reopen continues at 1024 visits without repeating cheap coverage. Wrong array lengths, out-of-range numbers, strings, and null payloads are rejected while a previous usable evaluation remains intact. The worker/subprocess test verifies maps crossing background snapshots and returning from the cache with engine/model paths unavailable.

Fifteen temporary mutations each caused a behavioral test failure and were restored: disabling the overlay, forcing it on, reversing White's perspective, merging diagonal neighbors, removing chain averaging, drawing internal boundaries, making every outline solid, making every outline broken, keeping parent ownership, disabling the request flag, dropping response ownership, omitting ownership from serialization, accepting wrong lengths, accepting out-of-range values, and letting shallow results overwrite deeper maps. Mutation logs remain under ignored `local/`.

The explicit Metal KataGo test refined three synthetic 9×9 positions through 1, 8, 64, and 256 visits. Refinement responses contained 81 bounded ownership entries, exactly matching the parsed arrays; full evaluations and ownership survived the SQLite round trip. The one-visit coverage pass retained its previous request behavior and omitted ownership. Final candidate counts were 25, 10, and 11. This real-engine run took 6.29 seconds after compilation; it is protocol evidence, not an application speedup claim. The GPU-rendered test frame was generated and visually inspected using:

```sh
KATASTRO_GROUP_PREVIEW="$PWD/local/group-strength.png" cargo test --test group_strength toggle_colors_connected
KATASTRO_TEST_ENGINE=/opt/homebrew/bin/katago KATASTRO_TEST_MODEL="$HOME/katrain/katrain/models/b10c384h6nbttflrs.bin.gz" cargo test --release --test engine live_scheduler_refines -- --ignored --nocapture
```

Formatting, Clippy with warnings denied, and all 60 standard integration tests passed after restoring the mutations. The cache identity and deepest-result policy remain compatible with earlier chart data. Existing maps are reused offline; missing maps arrive on subsequent refinement. The display estimates expected ownership under continued play and does not claim a calibrated survival probability or a life-and-death proof. Native dialog handling is unchanged, so the prior explicit dialog evidence remains above.

## Fast ownership backfill and clearer group markers

The user found ownership slow to appear and orange/red outlines hard to see against the board. Five new routine integration tests bring the suite to 65, with an additional explicit real-engine latency benchmark. Before production changes, tests failed because a missing map requested 65,536 visits instead of a bounded pass, lower-visit ownership was discarded behind deeper chart values, a completed map could not backfill a deep cached chart, no opaque status rings were drawn, and the group legend falsely used the chart's 16,384 visits. An existing selected-variation test was strengthened: reverting interactive one-visit ownership reproduced its missing-payload failure before restoration.

Ownership now has a separate actual-visit count. The selected position receives ownership with an interactive one-visit result, and cached deep charts with missing maps get a prioritized 64-visit request. Streaming maps display independently of chart depth. Points, winrate, and candidates retain their highest chart visits; ownership retains its highest map visits. Persistence merges completed components in an immediate SQLite transaction, rejects sub-64 ownership even when carried with deep chart values, and preserves a prior deep map when later chart replies lack ownership. The scheduler returns original response values for final-result caching rather than merged transient display state. Metadata-free legacy maps infer their original root depth. Group labels show actual map visits and identify sub-64 previews as quick estimates.

The user selected opaque colored rings on stones plus dark halos around chain outlines. UI tests compare actual ring colors and full opacity for Black/White pairs of all three statuses, require matching wider dark strokes beneath red/orange solid and dashed boundaries, exercise legal variation input, and verify unchanged board bounds when a quick-map legend becomes a 64-visit legend. A GPU-rendered preview was inspected using `KATASTRO_GROUP_PREVIEW="$PWD/local/group-rings-and-halos.png" cargo test --test group_strength toggle_colors_connected`.

Fourteen deliberate regressions caused behavioral test failures before restoration: uncapped missing-map requests, omitted interactive one-visit ownership, dropping lower-visit maps, downgrading charts, coupling map depth to chart depth, replacing deep maps with cheaper late maps, passing merged transient state to cache writes, ignoring cache backfill, persisting cheap carried maps, displaying chart depth in the group legend, removing rings, reducing ring opacity, removing solid-outline halos, and removing dashed-outline halos. Logs remain under ignored `local/`.

### Measured latency

The benchmark uses the real background worker, local Metal KataGo 1.18.2, the same b10 model, and two private 19×19 games with 232 and 285 positions. It uses actual legacy cache results at 4,096 visits, rather than inventing chart values. A read-only snapshot of those results was frozen in ignored local storage, so before/after measurements use the same games, selected nodes (116 and 142), profile, and cached depth. Both versions warm the engine before measurement; the clock runs from OpenAndAnalyze to the first ownership-bearing snapshot for the selected position. Three trials per game produced:

| Game positions | Before, three trials | After, three trials |
| --- | --- | --- |
| 232 | 22.557 s, 22.542 s, 29.069 s | 0.394 s, 0.384 s, 0.395 s |
| 285 | 17.374 s, 19.218 s, 23.104 s | 0.375 s, 0.370 s, 0.375 s |

Across all six observations, median readiness fell from 22.550 s to 0.380 s. Earlier maps used roughly 4,100 visits; the fast displayed maps used 35–38 visits and are explicitly provisional. Every measured snapshot retained the original 4,096-visit chart or a deeper result. These are warm local worker measurements, not an accuracy equivalence claim or a guarantee including cold model startup. The original SGFs and production databases were read-only; imports, selections, cache writes, and analysis ran in temporary storage.

Reproduce the new measurement with a read-only legacy cache containing analyzed positions without ownership:

```sh
KATASTRO_TEST_ENGINE=/opt/homebrew/bin/katago KATASTRO_TEST_MODEL="$HOME/katrain/katrain/models/b10c384h6nbttflrs.bin.gz" KATASTRO_SGF_DIR="$HOME/Downloads" KATASTRO_LEGACY_CACHE="$PWD/local/group-latency-fixtures/analysis.sqlite" cargo test --release --test group_strength_latency -- --ignored --nocapture
```

Both explicit real-engine tests passed: one-visit requests returned board-sized bounded ownership maps, and three positions refined through 1/8/64/256 visits with exact deepest cache round trips. Final candidate counts were 25, 11, and 18; protocol checks ran in 5.43 seconds after compilation. Native dialog handling is unchanged; prior dialog evidence remains documented above.

Final formatting, Clippy with warnings denied, and all 65 standard integration tests passed after restoring the counterfactual changes. The packaged app is rebuilt from the verified implementation.


## Untinted stones and thinner group boundaries

The user found that alive groups were visually too similar when stone fills were tinted and requested thinner group borders. Two new integration tests were written before the drawing changes. The first compared actual stone fills with Group strength off and on, and failed on an extra translucent green fill at B8. The second failed on the oversized colored boundary. The tests use real SGF parsing, actual toggle and board clicks, and temporary review databases; they cover both stone colors across live/unsettled/dying readings, preservation of strength outlines, and reopening an autosaved variation.

The overlay now leaves stone fills unchanged. Opaque inscribed status rings and exposed chain boundaries carry strength colors. Colored chain boundaries shrink from 2.5 to 1.5 points, and their dark contrast halos from 5.5 to 3.2 points. Existing overlay tests now detect colored rings rather than depending on the removed tint, preserving their chain-averaging, perspective, navigation, pending-map, and board-input assertions. The tooltip and current product documentation describe the new treatment.

Four temporary regressions were detected and restored: reintroducing all stone tints, tinting only White stones, restoring the thick colored border, and restoring the thick halo. Red/green and mutation logs are in ignored local storage. The routine suite now contains 67 tests. The previous real-engine and latency evidence remains applicable because this follow-up changes only rendering and its tests.


Formatting, Clippy with warnings denied, and all 67 standard integration tests passed after restoring the rendering mutations. An explicit preview attempt (`KATASTRO_GROUP_PREVIEW="$PWD/local/group-no-tint-thin.png" cargo test --test group_strength toggle_colors_connected`) could not create a GPU render state in the current restricted session: `No adapter found`. The ordinary headless rendering/input tests passed; the newest appearance has no GPU screenshot evidence from this session.


## Visual-only AI hints and slimmer inscribed rings

The user requested a default-on AI suggestion toggle beside Group strength, keeping ongoing background analysis active, and asked to halve the inscribed strength rings. Three new UI integration tests and a strengthened ring assertion were written before production behavior. After adding only the checkbox interface, the tests failed on candidates still being painted, a played marker still filled by an overlapping candidate, and board hints remaining visible during a controlled engine run. The ring test failed because its old maximum width was four points. These red runs were behavioral failures. The sidebar input fixture uses a tall window to ensure the clicked row is visible, and empty-intersection checks exclude the board's small star points.

AI moves starts enabled and changes only board drawing. Tests check actual filled circles and delta text, adjacent toggle placement, stable board bounds when switching, continued group markers, navigation and fresh-analysis updates retaining the off setting, clickable sidebar moves, and reopening the resulting saved variation with its original main line intact. Both Black and White played-move markers retain their hollow outlines and exactly one signed point delta when a matching AI suggestion is hidden, and regain their blue candidate fill when restored.

The background test uses the real worker and a controlled fake engine subprocess. A named-pipe barrier holds the first 256-visit request after complete 64-visit coverage. The actual UI click emits no review or engine commands; releasing the barrier produces completed deeper results and full coverage while the application remains running and board hints remain hidden. Synchronization uses channels and bounded deadlines, with no arbitrary sleeps. Sidebar suggestions still render after the update.

Inscribed rings shrink from 2–4 to 1–2 logical points, exactly half their prior width. They retain opaque, identical status colors across Black and White stones. The untinted stone fills and the thinner external group boundaries remain covered by the preceding tests.

Nine temporary regressions were detected and restored: defaulting hints off, ignoring the drawing toggle, resetting it every frame, suppressing the played-move score, pausing analysis, restarting analysis, letting Group strength re-enable hints, hiding the recorded-move ring, and restoring thick inscribed rings. Logs remain under ignored local storage. The standard suite now contains 70 integration tests. The previous real-engine protocol and latency results still apply; this follow-up changes GUI rendering and the controlled test fixture. The current session still exposes no GPU adapter for an explicit screenshot preview.


After restoring all nine mutations, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` passed; the latter ran all 70 routine integration tests with zero failures. Final logs are in ignored `local/board-ai-toggle-half-ring-final.log`.


The release bundle was rebuilt with `bash scripts/package-macos.sh "$PWD/local/Katastro-next.app"`. The installed `local/Katastro.app` passed `codesign --verify --deep --strict --verbose=2`; its binary contains the new AI moves control. The previous app was retained as ignored `local/Katastro-23637d8.app`. The running application was not terminated or relaunched.

## Suggestions after successive variation moves

The user reported missing variation hints, then found them working in another game. Investigation reproduced a specific queueing case rather than establishing that every variation was affected: the original-game one-visit coverage request and a previous interactive one-visit request could fill both slots. Moving again retained both, leaving the newest variation without analysis until older work finished.

Two integration tests were written before changing production behavior. The application/scheduler test failed because no request could be issued for the newest branch. The UI/worker test held those two older requests in a controllable fake subprocess, clicked D6 and E5 on the actual board, and timed out with selection 3, no evaluation, and original coverage 0/2. Both failures reached the intended missing behavior with working SGF, SQLite, UI, and engine fixtures.

Selection now cancels superseded interactive work even at one visit, preserving the original-game quick coverage request. The tests obtain scored 64-visit suggestions before original coverage finishes, verify a blue F5 marker with a +1.5 point delta, play it through the sidebar, and reopen the persistent branch and cached suggestion with the imported main line intact. The core test then completes the original chart using its original request. Late `noResults` for the canceled branch cannot prevent the new hints. Synchronization uses explicit diagnostic barriers and bounded channel deadlines, with no arbitrary sleeps or GPU timing assertions. One-visit policy previews still stay off the board until point estimates arrive.

Three deliberate regressions were detected and restored: retaining the obsolete one-visit interactive request (also detected through UI/worker input), canceling original coverage, and omitting the selected position's scored follow-up. The over-cancellation mutant initially survived; the test was strengthened to require completion of the chart from its original request, and that mutant then failed. These checks establish outcomes beyond request counts. Logs are in ignored `local/variation-hints-*.log`.

After restoring those mutations, formatting, Clippy with warnings denied, and all 72 routine integration tests passed. Both explicit real-engine tests were attempted again with the documented executable/model and exited before returning any analysis. A direct run with the application's identical config confirmed `Metal backend: Failed to create Metal device` and failure to create the MPSGraph handle in the current restricted session. The prior live-engine successes above remain historical evidence; this repair has controlled-subprocess and headless UI evidence, without a new live-engine pass. Logs are in ignored `local/variation-hints-final.log`, `local/variation-hints-real-engine.log`, and `local/variation-hints-engine-diagnostic.log`.

The release app was rebuilt, verified with `codesign --verify --deep --strict --verbose=2`, and installed as ignored `local/Katastro.app`. Its previous bundle is retained as `local/Katastro-before-variation-hints.app`. No running app was terminated or relaunched.

### Verification after lifting session restrictions

Before publishing, the session gained unrestricted filesystem and network access. Both explicit real KataGo tests then passed with the documented engine and model: three positions refined through 1/8/64/256 visits, final candidate counts were 29/10/13, and deepest completed results round-tripped exactly through SQLite. The run took 6.67 seconds after compilation. This resolves the earlier Metal initialization limitation for the final implementation; the failed restricted attempts remain recorded above.

The latest untinted stones, thin chain boundaries, and halved inscribed rings also rendered successfully on the GPU. The preview was inspected after `KATASTRO_GROUP_PREVIEW="$PWD/local/group-final-no-tint-thin.png" cargo test --test group_strength toggle_colors_connected -- --nocapture` passed. The image and logs remain in ignored local storage; the figure uses a synthetic SGF. No app implementation changed during these final checks.

## Simplification and correctness pass

The pass reviewed the SGF/rules/tree, cache, scheduler, engine transport, worker, desktop/dialog integration, and board/chart rendering. Four new integration tests reached behavioral failures before their fixes:

| User benefit | Observed failure | Passing outcome |
| --- | --- | --- |
| Export leaves the source SGF intact | Export to a hard-link alias replaced the original bytes | Original, symbolic-link, and hard-link targets are rejected; a separate export retains the variation |
| New Zealand games follow the engine's rules | A legal two-stone suicide was rejected by positional superko | Different next-player states remain legal, identical situations are rejected atomically, and the branch reopens correctly |
| The played line survives child reordering | Actual UI preview followed the variation; a later export assertion exposed a changed primary game | Real UI navigation, preview, top-row layout, persistence, and exported final board follow the frozen line; variation continuation still works |
| Storage errors appear without extra user input | The worker published an error but sent no UI wake notification | Startup failure supplies both the error snapshot and an explicit wake event |

The reordered-tree fixture creates its branches through Review commands, then reorders only the persisted child list while retaining frozen main-line IDs. The UI test opens that real SQLite document through the worker, clicks Next and tree nodes, and reopens/exports it. The New Zealand check matches `~/KataGo/cpp/game/rules.cpp`'s situational ko and multi-stone suicide settings. All fixtures are synthetic; synchronization uses channels and bounded deadlines without arbitrary sleeps.

Continuation ordering, cache-key hashing, and snapshot notification now have shared implementations. The unused recursive OpenAndAnalyze handling was folded into the normal open path; cached snapshots still publish before engine startup. The cache-key serialization/version is unchanged. The README shrank from 1,139 to 183 words, with detailed test invocations moved into this document.

Ten temporary regressions failed their integration checks before restoration: checking source aliases by pathname, using positional NZ ko, ignoring NZ superko, selecting continuation by child order, laying out by child order, exporting by child order, navigating by child order, previewing by child order, desynchronizing scheduler/persistence cache keys, and omitting startup-error wakes. Logs are in ignored `local/code-pass-*.log`.

Final formatting, Clippy with warnings denied, and all 76 routine integration tests passed. Both explicit real Metal KataGo tests passed: three positions reached 256 visits with final candidate counts 25/7/14 and exact deepest cache round trips. The read-only corpus check replayed 65 SGFs and 12,334 positions with no failures (2.034 seconds); this is an import/replay measurement, not an analysis speedup. Original game files, engine sources, and production databases were unchanged.

The release bundle was rebuilt and passed strict/deep code-signature verification before replacing ignored `local/Katastro.app`. The previous bundle remains at `local/Katastro-before-code-pass.app`; running applications were not terminated or relaunched.

## Full-height board and review sidebar

The user's OGS screenshots guided the board-first layout. The application header and full-width footer are removed. Players, file actions, board toggles, charts, move buttons, keyboard shortcuts, status, and the variation tree now occupy the right column. Its tree and navigation remain visible while analysis details scroll. The board uses the largest square that fits the remaining full-height area, with narrow margins; neither status changes nor arriving group estimates shrink it.

Two new integration tests exercise real SGFs, temporary SQLite review stores, and actual pointer/keyboard input at 1600×900, 1200×860, and 900×680. Before implementation, the layout test failed because player controls occupied the board area. The long-name test then failed because a player label spilled out of the sidebar. Subsequent assertions exposed a 20-point shift when ownership arrived and a painted error clipped by the status panel. Fixed legend space and truncated labels with full-text hover help resolved those failures.

The tests place and select persistent branches from the relocated buttons/tree, play a candidate after scrolling the narrow analysis column, resize the window, verify retained toggle state, and reopen the review with its original main line intact. The UI adapter now carries the actual source path. That exposed winrate clipping at the default size; the strengthened chart check compares against the fixed navigation buttons, and more compact charts made it pass. Existing navigation, candidate, chart, group, controlled-worker, and dialog checks remain in the regression suite.

Ten temporary mutations produced behavioral failures and were restored: adding a global header, reserving old move-bar space on the board, moving the sidebar below the board, suppressing move-button commands, suppressing tree selection, resetting the AI toggle, removing player truncation, dropping the fixed group-legend height, clipping long status text, and enlarging charts beneath the fixed controls. Logs remain in ignored `local/layout-*.log`.

GPU-rendered wide-layout and actual desktop/worker previews were generated and visually inspected. They use synthetic games and supplement the input/persistence assertions. Reproduce them in a graphical macOS session with a Metal adapter:

```sh
KATASTRO_LAYOUT_PREVIEW="$PWD/local/layout-wide.png" cargo test --test ui height_filling_board -- --nocapture
KATASTRO_UI_PREVIEW="$PWD/local/layout-desktop.png" cargo test --test desktop -- --nocapture
```

Final formatting, Clippy with warnings denied, and all 78 routine integration tests passed after restoration. The explicit native Open/Export sheet regression also passed. Engine, scheduler, and cache behavior are unchanged; this layout pass does not claim new engine performance measurements.

The release bundle was rebuilt and passed strict/deep code-signature verification before replacing ignored `local/Katastro.app`. The previous bundle remains at `local/Katastro-before-sidebar.app`; no running app was terminated or relaunched.

## Import and analysis preparation audit (November 2026)

The audit covered SGF decoding and rules replay, review persistence, cache identity, request scheduling, engine transport, worker snapshots, and desktop rendering. Two repeated-work costs were addressed:

- Import previously reconstructed the board from the root for every node. It now validates each tree edge once, copying board and ko history only at forks. Setup application and move application are shared with selected-board replay. All sibling variations are still validated before the imported review replaces the current document.
- Preparing each analysis position previously cloned the complete document to clear the root move. It now constructs the initial board directly from handicap, setup stones, and explicit player properties, keeping any root move in the move history. Position serialization, cache keys, source identity, and saved-review formats are unchanged.

Full-document snapshot copies and repeated model/executable hashing on Analyze remain potential follow-ups. This pass did not change them: they need separate worker/UI allocation measurements and careful invalidation behavior. No inference or UI frame-time speedup is claimed here.

### Evidence and measurements

Before implementation, an explicit release benchmark recorded a baseline and a second unchanged run failed the required 25% reduction. It uses the same 413 private local SGFs (80,043 positions), verifies their aggregate content fingerprint, and reports the median of three passes. Inputs are already in memory: the timed import phase includes parsing and legal-tree validation; the second phase constructs every analysis position. Real SQLite imports, final-board selection, and replay are also exercised outside the timed sections. Original SGFs are read-only and neither names nor game content are committed.

| Metric, whole corpus | Baseline | After |
| --- | ---: | ---: |
| Parse and validate | 13.169 s | 0.201 s |
| Prepare all analysis positions | 3.158 s | 0.681 s |

Measured on Apple M3, macOS 26.6.2, Rust 1.101.0-nightly, release profile. These are local CPU measurements, approximately 65.6× and 4.6× respectively; they do not include file dialogs, database lookup latency, model startup, or KataGo search. Timing assertions run only in the explicitly invoked benchmark, never in routine tests.

Two new characterization integration tests passed before the refactor and stayed green afterward. They cover sibling ko isolation, rejection of an illegal non-main-line branch without replacing the current review, setup-history reset, persistent variation selection, root setup plus a root move, explicit White-to-play, identical equivalent engine histories, and per-position cache round trips. Temporary mutations that skipped sibling validation, retained pre-setup history, or treated the root move as setup each produced behavioral failures before restoration. Restoring repeated import-prefix replay raised import time to 12.864 s and failed the import benchmark; restoring whole-document cloning raised position preparation to 3.042 s and failed its separate metric. Both optimizations were restored afterward. Raw logs and timings remain under ignored `local/`.

Reproduce the baseline on the old implementation, then compare on the new implementation (same compiler/profile and unchanged corpus):

```sh
mkdir -p local
KATASTRO_SGF_DIR="$HOME/Downloads" KATASTRO_PERF_RECORD=local/import-baseline.json cargo test --release --test import_performance -- --ignored --nocapture
KATASTRO_SGF_DIR="$HOME/Downloads" KATASTRO_PERF_BASELINE=local/import-baseline.json cargo test --release --test import_performance -- --ignored --nocapture
```

### Fox investigation

The supplied Fox SGF imported and all 213 positions returned real KataGo analysis through `Client::OpenAndAnalyze` **before any production change**. The full private corpus also imported and replayed successfully before the refactor. Its duplicate `AP` properties, versionless `AP[foxwq]`, UTF-8 names without `CA`, `KM[0]`, and `HA[1]` are already accepted. The explicit zero komi and Black-to-play are retained. No speculative Fox rules/komi correction was made, and no Fox-specific parser failure was reproduced; the user's app symptom/error is still needed to identify that reported problem.

A small synthetic Fox fixture now covers the public Review API, temporary SQLite databases, and a controlled fake subprocess through 1/8/64-visit coverage and refinement. It checks player text, final board, unchanged original game, saved variation, SGF export, exact cache restoration, and omission of the cheap pass after reopening. The same fixture has an explicit real-engine test. A separate opt-in test reads the user's supplied path and checks full chart coverage through the background worker with real KataGo, without embedding the private file or its moves.

```sh
KATASTRO_TEST_ENGINE=/opt/homebrew/bin/katago KATASTRO_TEST_MODEL="$HOME/katrain/katrain/models/b10c384h6nbttflrs.bin.gz" cargo test --test fox live_fox_analysis_and_cache -- --ignored --nocapture
# Set KATASTRO_TEST_SGF to the private Fox SGF's full path.
KATASTRO_TEST_ENGINE=/opt/homebrew/bin/katago KATASTRO_TEST_MODEL="$HOME/katrain/katrain/models/b10c384h6nbttflrs.bin.gz" cargo test --test fox live_local_fox_worker_import_and_analysis -- --ignored --nocapture
```

Both explicit tests require a working local KataGo executable and model, with Metal access for this installed binary. They do not use production review/cache databases. The local `~/KataGo` reference checkout was absent during this pass; the installed engine and `~/katrain` reference remained read-only.

Final verification: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` passed (81 routine tests; nine opt-in tests excluded by default). Both existing real KataGo tests and both new live Fox tests passed on the restored implementation. The scheduler reached 256 visits with exact cache restoration; the private Fox file again reached 213/213 analyzed positions. The final repeated performance check passed at 0.199 s for import and 0.708 s for position preparation. The release app was rebuilt at ignored `local/Katastro.app` and passed `codesign --verify --deep --strict`. Live AppKit sheet and optional GPU screenshot checks were not rerun for these core-only changes; their existing invocations and prerequisites above remain applicable.

## Fox opening point balance correction (2026-11-09)

The newly supplied even-game SGF exposed a different Fox import bug: its `KM[375]` was sent literally to KataGo, producing **−203.397 points at one visit** on the empty board. This is the [reported Fox komi encoding issue](https://github.com/SabakiHQ/sgf/issues/11); the previous zero-komi fixture could not expose it. Import now recognizes root Fox application metadata, including duplicate AP properties in either order, and converts the known Chinese `375`/`325` and Japanese `650` encodings to point komi. Already normalized values, explicit zero/handicap komi, unrelated applications, and unrecognized values remain unchanged. The source SGF remains byte-for-byte unchanged; exports contain standard komi.

The observable benefit is a meaningful opening chart and reusable analysis for Fox games. A regression that passes the encoded value through would restore the extreme score. A separate persistence risk is losing saved variations when corrected komi changes the document identity, or displaying old analysis if the saved document retains 375-point komi. Import recovers the legacy review when no corrected review exists, retains its variations, selection, and frozen main line, and saves under the corrected identity. Old analysis stays under its incompatible komi key; it is never copied into the corrected cache.

### Test evidence

Three new integration tests in `tests/fox.rs` failed behaviorally before production changes: the scheduled request contained 375 instead of 7.5, the legacy −200-point cache populated the chart, and the encoded-komi compatibility cases failed. They now pass through real SGF parsing, temporary SQLite databases, application commands, and the controlled fake engine. They cover 1/8/64-visit coverage/refinement, cache restoration without a repeated cheap pass, persisted multi-move variations and selection, legacy review recovery, rejection of an incompatible 16,384-visit cached score, standard SGF export, idempotent reimport, and unchanged source bytes. The metadata matrix also checks duplicate/versioned AP, ordinary non-Fox komi, comment text resembling AP, and zero/handicap values.

Four temporary mutations each caused a behavioral test failure: passing through 375, disabling the legacy review lookup, restoring the encoded komi on the saved document, and exporting the encoded value. All mutations were restored, and the focused suite passed afterward. Logs are under ignored `local/fox-komi-*.log`.

The explicit local-file test also failed against real KataGo before the fix with −203.397 points. Afterward, the same file reached **150/150 analyzed positions** and its opening evaluated to **−0.796 points at 64 visits**, with komi 7.5. These are observed search estimates, not fixed expected scores or a performance claim. The live synthetic Fox test now covers both zero komi and encoded 7.5 komi, including persisted analysis on reopen.

Final checks passed: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` (84 passed; nine opt-in tests excluded by default). Both live Fox tests, both existing live engine tests, and the private local SGF corpus import/replay check passed explicitly. The release bundle at `local/Katastro.app` was rebuilt and passed `codesign --verify --deep --strict`. Live AppKit/GPU checks were not rerun for this import/persistence change; their prerequisites and invocations remain documented above. No private game files, databases, or engine assets are included in Git.

Reproduce with a working local KataGo executable/model and Metal access for this installed engine. The private local test requires an empty-board opening; it uses temporary review/cache databases and never modifies the input SGF.

```sh
cargo test --test fox
KATASTRO_TEST_ENGINE=/opt/homebrew/bin/katago KATASTRO_TEST_MODEL="$HOME/katrain/katrain/models/b10c384h6nbttflrs.bin.gz" cargo test --test fox live_fox_analysis_and_cache -- --ignored --nocapture
# Set KATASTRO_TEST_SGF to the affected private SGF's full path.
KATASTRO_TEST_ENGINE=/opt/homebrew/bin/katago KATASTRO_TEST_MODEL="$HOME/katrain/katrain/models/b10c384h6nbttflrs.bin.gz" cargo test --test fox live_local_fox_worker_import_and_analysis -- --ignored --nocapture
```


## Pandanet sync and game library (2026-11-12)

The user can connect Pandanet once, remember the login in macOS Keychain, and browse synced games directly from **Games**. Automatic checks run on startup and every 15 minutes while the app is open. The recent-history cap defaults to 200; increasing it backfills, and lowering it preserves older downloads. Two downloads can overlap. A malformed individual response is recorded for retry while other games continue. Originals and saved review variations remain separate.

Regressions would include fetching only the first archive page, re-downloading on each login, suppressing failed older records behind a known page, silently losing cancellation progress, blocking review input during a request, or requiring a file picker to open an already synced game. Tests exercise the public sync service with real temporary SQLite databases and SGF parsing, the threaded client with an injected clock, and actual egui input through DesktopApp. Synthetic local HTTP fixtures supply archive pages, authentication failures, redirects, malformed SGFs, and transient failures. Gates/channels control concurrent requests and startup shutdown; routine tests do not assert network speed or use arbitrary sleeps.

### Red, green, and mutation evidence

The initial service interfaces compiled before the behavioral implementation: pagination/persistence, scheduling, cancellation/retry, authentication errors, session-only login/forgetting, and redirects failed through missing downloads or accepted invalid responses. The initial background worker did not publish a completed sync, the desktop had no Pandanet action, and the Keychain stub returned no saved password. Each test passed after its production behavior was implemented.

Follow-up failures established the requested changes: the history-cap test downloaded three games instead of two; gated downloads admitted only one request before release; an HTML response prevented the other valid game from being saved; and the library test could not find Games. Additional red tests exposed automatic-sync preference loss after interruption, failed manual-login recovery, an older failed record hidden by a known page (three games instead of four), and a fresh request starting after the app closed during worker startup. Those cases are now green. A characterization test also confirms that changing the session-bearing links causes zero duplicate transfers and deleting one original causes exactly one replacement transfer, which imports successfully.

The original 11 temporary mutations were detected behaviorally: one-page pagination, duplicate fetches, ignoring the timer deadline, ignoring cancellation, disabling automatic sync after cancellation, accepting malformed archive HTML, skipping SGF validation, disabling the daily sweep, failing to clear login-needed state, omitting Keychain save, and removing the desktop entry. Ten final mutations checked the revised implementation: ignoring the cap, serializing the concurrent batch, aborting after a bad game, removing the Games action, ignoring search text, skipping older failed retries, allowing a sync after startup shutdown, duplicate fetches, ignoring missing files, and including the session token in game identity. Every mutation caused an outcome assertion to fail and was restored before final checks. Final mutation logs are in ignored `local/pandanet-mutation-*.log`.

### Authenticated and native evidence

The user signed into the Pandanet website and the native preview locally, then confirmed that the native download started. The initial build saved 34 actual originals and remembered the login preference. Its generic invalid-SGF failure could not be reconstructed because that body was discarded. Retrying the next missing record through the authenticated browser returned a valid 229-move SGF, which passed the public Review import/replay check using temporary databases. This supports retrying transient responses; it does not prove the original response's exact cause. Browser inspection also verified a second archive page, matching record identities across independent sessions, and a session-bearing SGF URL working without cookies.

The disposable macOS Keychain test saved, reloaded, updated, and deleted its own test item. It does not inspect the user's password. UI integration tests sign in, remember the login, keep review navigation and variation creation usable while HTTP is gated, then open a synced game. The Games test filters two records by date, selects the intended record, and checks the loaded SGF and intact main line. An optional GPU render of that synthetic library was inspected; the interaction assertions, not the screenshot alone, establish behavior.

A six-game live download spot check used the same authenticated archive links and fresh temporary stores for each mode:

| Mode | Elapsed | Imported games | Total SGF bytes |
| --- | ---: | ---: | ---: |
| Serial with the original one-second spacing | 12.244 s | 6 | 19,871 |
| Two concurrent transfers, no artificial spacing | 1.807 s | 6 | 19,871 |

The games contained 50–260 moves. This measures HTTPS downloads, SGF validation, and local persistence for that sample; authenticated listing was supplied by a localhost fixture and excluded. There was one run per mode, with serial first, so server/cache/network effects may contribute. It is evidence for this spot check, not a promised general speedup or optimal rate. The bounded-overlap integration test separately establishes concurrent behavior without a timing assertion. Live HTML/SGF responses and session URLs remain outside Git.

Reproduce the focused checks and optional native checks:

```sh
cargo test --test pandanet --test pandanet_background --test pandanet_ui
# macOS login Keychain; creates only a disposable test item and may prompt for access.
cargo test --test pandanet_keychain -- --ignored
# Private SGF downloaded through an authenticated Pandanet session; input is read-only.
KATASTRO_PANDANET_SGF=/path/to/private/game.sgf cargo test --test pandanet_live live_pandanet_download_imports_and_replays -- --ignored
# Optional synthetic GPU preview; requires a working wgpu adapter.
KATASTRO_PANDANET_UI_PREVIEW=local/pandanet-library.png cargo test --test pandanet_ui games_library_filters
# Live benchmark: private HTML with exactly six authenticated SGF links, whose session must still be valid.
# This downloads that six-game sample twice. Do not commit the HTML or URLs.
KATASTRO_PANDANET_ARCHIVE_HTML=/path/to/private/archive.html KATASTRO_PANDANET_USERNAME=your-account cargo test --test pandanet_live live_download_batch_compares_serial_pacing_with_two_parallel_transfers -- --ignored --nocapture
```

The HTML archive is an undocumented interface. Controlled fixtures cannot guarantee future compatibility with Pandanet; the authenticated checks above supplement them. Real KataGo and AppKit sheet checks were not rerun for this sync/library change; their existing prerequisites and explicit invocations above remain applicable.

Final automated checks passed: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` (103 passed; 12 opt-in tests excluded by default). The disposable Keychain test also passed explicitly. The release bundle at ignored `local/Katastro.app` was rebuilt and passed `codesign --verify --deep --strict`. The rebuilt app migrated the existing sync configuration to the 200-game default and requested macOS Keychain access for its changed local signature.

After the user granted the rebuilt app Keychain access, native automatic sync resumed from the existing 34 records and completed at exactly 200 games. The sync index recorded a completed check with zero failed records, and all 200 local SGFs matched their stored SHA-256 values. No password was retrieved by diagnostic tools or written into repository artifacts.


## Sortable Pandanet library table (2026-11-12)

The library now presents one field per column: date, White, White rank, Black, Black rank, winner, and win condition. Users can sort each column in either direction, resize the columns, search normalized result text, and click a row to open its game. Newest-first is the default; ranks order by Go strength and point margins numerically. Resignation/time wins and unrecognized result text remain readable. This uses metadata already stored by sync; it requires no re-download or database migration.

The observable benefit is comparing games by a chosen field and opening the intended record after sorting. Plausible regressions include cosmetic headers that do not reorder rows, alphabetical ordering of 10.5 versus 2.5 points or 10k versus 2k, combined name/rank cells, and click handlers still using the original unsorted row index. The new `library_table_sorts_every_column_and_opens_the_correct_game_after_filtering` integration test was written and run first: it failed because the real UI had no **Sort by Date** action. After implementation it verifies all seven columns in both directions through actual header clicks, rank/margin ordering, separate aligned player cells, winner/condition text, fallback text, filtering by the normalized **Resignation** label, and opening that row's actual SGF with its frozen main line. Existing sign-in, non-blocking sync, library search, and direct-opening UI tests also pass.

A synthetic GPU render was inspected for table layout, readable text, and the active direction marker. The interaction outcomes establish behavior; the render supplements them. Reproduce the focused check and optional render (the latter requires a working wgpu adapter):

```sh
cargo test --test pandanet_ui
KATASTRO_PANDANET_TABLE_PREVIEW=local/pandanet-table.png cargo test --test pandanet_ui library_table_sorts_every_column
```

Seven targeted mutations each produced a behavioral failure: ignoring the selected column, preventing direction reversal, sorting point margins as text, sorting ranks as text, replacing resignation with an incorrect condition, opening the original first record instead of the clicked sorted record, and keeping player/rank text combined. All mutations were restored. Logs are under ignored `local/pandanet-table-mutation-*.log`.

Final verification passed: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` (104 passed; 12 opt-in tests excluded by default). The optional synthetic GPU table render also passed and was inspected. The optimized bundle at `local/Katastro.app` was rebuilt and passed `codesign --verify --deep --strict`. Native Keychain, live Pandanet transfers, real KataGo, and AppKit file-sheet checks were not rerun for this display-only change.


## Compact ranks, win markers, and library resizing (2026-11-13)

The table now has five sortable columns, with ranks directly beside the White and Black player names. Player columns retain name-based ordering. A small gold vector trophy before the date marks wins by the synced account, whether that account played White or Black. Losses and unknown outcomes get no trophy. The compact marker and inline ranks take inspiration from the user's supplied OGS game-history page, inspected read-only; no profile or game content was copied into fixtures. The table now fills the available vertical space when the library window is resized, including with sync settings open. This supersedes the separate-rank-column layout above.

The existing public UI integration test was updated first and failed because separate rank headings remained. A subsequent pre-implementation run found zero win markers instead of two. The strengthened test now asserts combined name/rank cells, all five sortable columns in both directions, visible gold trophy geometry on the correct two rows, no markers for losses/unknown results, normalized search, and opening the intended actual SGF after sorting/filtering. The new resize integration test first reproduced the fixed-height restriction: dragging the window left its height and 12 visible rows unchanged. It now verifies a stable initial size, performs an actual pointer drag on the resize corner, observes more visible rows (8 to 20 in the checked run), opens settings without restoring the old 240-pixel cap, and opens a newly exposed game through the existing review worker. Screenshots of the synthetic compact and enlarged views supplement those outcome checks.

```sh
cargo test --test pandanet_ui
# Optional GPU renders require a working wgpu adapter; all content is synthetic.
KATASTRO_PANDANET_TABLE_PREVIEW=local/pandanet-inline-trophy.png KATASTRO_PANDANET_TALL_PREVIEW=local/pandanet-height.png cargo test --test pandanet_ui
```

Four temporary mutations were detected by behavioral failures: hiding inline ranks, putting trophies on non-wins, making the painted trophy invisible while retaining its accessible label, and restoring a 400-pixel body cap. All were restored before final checks. `cargo fmt --all --check`, strict workspace Clippy, and `cargo test --workspace` passed (105 tests; 12 opt-in checks excluded by default). Synthetic GPU views were inspected; native Keychain, live Pandanet transfers, real KataGo, and AppKit sheets were not rerun for these UI changes.

The pre-push credential audit used local Gitleaks 8.30.1 with redaction and allow-comments disabled, scanning all reachable Git history and an isolated copy of all 51 tracked working files plus commit messages. Both scans reported zero findings. A separate inspection of all 165 existing historical blobs checked Pandanet session-bearing URL literals, literal login-form passwords, embedded URL credentials, and private artifact paths. The only matches were explicit synthetic fixture values; no captured game files, archive responses, SQLite databases, private keys, or client packages were tracked in any revision. The outgoing login code reads credentials at runtime from Keychain or local UI input; diagnostic tools did not retrieve the real password. Audit reports and screenshots remain in ignored `local/`. The final committed tree and history are scanned again immediately before pushing.

The optimized `local/Katastro.app` bundle was rebuilt and passed `codesign --verify --deep --strict`.


## Organization and duplication audit (2026-11-13)

This behavior-preserving cleanup follows the library refinements. The audit identified repeated Pandanet desktop setup, duplicated settings serialization and SQL in the worker, the 1,417-line rendering module, and repeated caption, chart conversion, group-outline, and table-cell formatting code. Shared test setup preserves the individual HTTP fixtures, input sequences, synchronization, and outcome assertions. Review now encapsulates settings persistence and its database connection; private board/chart/sidebar/tree modules separate rendering responsibilities. Existing public entry points, database formats, UI identifiers, drawing order, and sync policy are retained. No engine protocol or Go-rule changes were needed.

Relative to `53c08a8`, Rust source falls from 5,447 to 5,430 lines and tests from 7,942 to 7,847 lines: 112 fewer lines overall, including new imports for module boundaries and a stronger assertion that a restarted worker restores the saved engine path instead of its fallback. No tests or outcome assertions were removed. Counts include blank lines and all `.rs` files below `src` and `tests`; this is maintainability evidence, not a performance claim.

The 14 worker/cache integration tests pass before and after settings consolidation. Independently redirecting engine and profile reads to a missing key makes the public restart workflow fail its saved-path or cached-coverage assertion; both mutations were restored. The 44 board/chart/group-strength/library UI tests also pass after extraction and simplification. These refactors retain existing behavior and its prior red/green evidence; the added settings assertion passes against both implementations.

Counterfactual checks also removed inline rank text, dropped zero-valued chart assessments, and replaced group-outline contrast halos with foreground strokes. Each caused its existing interaction/painting outcome test to fail. All three mutations were restored; the logs and mutation summaries remain under ignored `local/`.

Final checks: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` pass (105 tests; 12 opt-in tests excluded). `cargo check --no-default-features --lib` also passes, confirming that the core still builds independently of the desktop. The optimized app was rebuilt with `scripts/package-macos.sh` and passed `codesign --verify --deep --strict local/Katastro.app`. Real-engine, native-dialog, live-login, Keychain and GPU snapshot checks were not rerun for this refactor; UI outcome checks use actual egui input and inspect painting commands without GPU timing assertions.

The pre-push audit again scans all reachable history, exported committed files, and commit messages with Gitleaks 8.30.1 using redaction and disabled allow-comments. The staged 55-file tree and preceding 19 commits reported zero findings; project-specific checks found no session-bearing literals other than synthetic fixtures, embedded URL credentials, or private game/database/key artifacts in history. The final commit and its complete history are rescanned before pushing. Reports remain under ignored `local/`.


## Replenished analysis queue (2026-11-13)

Completed positions now free engine capacity without waiting for an entire batch. Compatible main-line positions still share queries; capacity counts unfinished turns, and variations can refine alongside deeper foreground work. Selection cancels only the affected turns and earlier foreground work. A selection below 64 visits also retires cheap background work targeting at most eight visits; deeper peers survive, except for one turn reclaimed when all eight slots are occupied. Quick foreground refinement and the first reusable 64-visit pass take precedence over admitting deeper replacement work. The worker fills available slots in one dispatch and validates unsupported positions once at startup. Engine configuration bytes and cache identities are unchanged.

Observable benefits are durable progress past a slow position, progress on independent variations during deeper foreground analysis, and retaining useful work while navigating. Two initial controlled-engine integration tests failed before implementation because no further work was supplied while a request remained unfinished. Both now drive the public worker through explicit loopback barriers, real SGF parsing and temporary SQLite stores; later positions and independent branches reach the cache, canceled 4,096-visit replies are rejected, and reopening restores the correct results, variations, board and selection. A third worker test initially fails because main-line slots are not coalesced; it now verifies cancellation of just one turn of a shared query and persistence of the other six turns plus the replacement selection. Branch cancellation explicitly checks the engine turn number rather than the unrelated tree node ID.

Public Review/scheduler tests also establish the priority boundaries. They first fail when quick selection admits replacement competitors, when deepening starts before the first reusable pass completes, and when cheap superseded work remains accepted. The passing tests preserve existing peer results, restore full capacity afterward, reject streamed-only completion of the first pass, and verify durable results on reopen. A supplementary full-queue scenario reclaims one position and completes the other seven. Nine temporary mutations were detected and restored: restoring the batch barrier, duplicating pending work, canceling all peers, omitting the foreground slot, admitting quick-refinement competitors, deepening prematurely, retaining cheap competitors, disabling coalescing, and canceling a whole shared query.

Final checks pass: `cargo fmt --all --check`, strict workspace Clippy, `cargo test --workspace` (112 passed; 14 opt-in checks excluded), and `cargo check --no-default-features --lib`. All three opt-in tests in `tests/engine.rs` and `live_fox_analysis_and_cache` pass against KataGo 1.18.2 / Metal with the b10c384h6nbttflrs model. These cover real policy/ownership output, refinement through 256 visits, ranked-suggestion persistence, komi, and cancellation of one turn while the other turns complete. The optimized app was rebuilt with `scripts/package-macos.sh` and passed `codesign --verify --deep --strict local/Katastro.app`. The running app was not restarted by the agent.

The [real-engine workload](performance.md) compares the previous scheduler at `df4f018` with this implementation on an Apple M3. Two private 19×19 game prefixes contain 97 positions each; a second phase adds 24 legal variations and makes eight selections. Each case uses fresh temporary stores and a warmed engine. Engine/model/configuration profiles and source hashes match. The user's independent analysis was paused, and no compilation ran during measurement. There is one measured run per version per game, so these results demonstrate this workload rather than a statistically established or hardware-wide speedup. Original SGFs and raw artifacts remain private.

| Case | All main-line positions at 64 visits (before → after) | All 24 variations at 64 visits (before → after) |
| --- | --- | --- |
| A | 63.54 s → 61.72 s | 29.10 s → 21.72 s |
| B | 73.59 s → 60.51 s | 21.12 s → 21.86 s |
| Mean | 68.57 s → 61.11 s | 25.11 s → 21.79 s |

Initial chart coverage averaged 2.40 s before and 2.42 s after. Across the 16 selections per version, first-estimate latency had a median of 38 ms before / 37 ms after and a maximum of 97 ms / 46 ms. Selected 64-visit latency had a median of 0.80 s / 0.72 s and a maximum of 1.80 s / 2.19 s. These are worker-snapshot timings, not displayed-frame timings. The longer worst-case refinement is a retained-work tradeoff; the first estimate remains available while search proceeds. Variation coverage improved in one case and regressed slightly in the other. Coarse system-wide GPU counters were recorded locally but do not prove saturation or maximum utilization. An earlier individual-query prototype did not establish an overall benefit; retaining coalesced main-line queries is part of the final implementation.

The pre-push credential audit uses Gitleaks 8.30.1 with redaction and disabled allow-comments on all history, an isolated export of the complete staged/committed tree, and commit messages. Project-specific checks also reject embedded URL credentials, unexpected Pandanet session literals, and private game/database/model/key artifacts in history. The staged 58-file tree and 21 preceding commits reported zero findings. Diagnostic tools did not retrieve login credentials. The final committed tree and complete history are rescanned immediately before pushing; reports remain under ignored `local/`.

## Lighter evaluation curves (2026-11-13)

Both score and winrate charts use thinner strokes: the played/shared line is 1.5 points (previously 2.0), the highlighted variation is 2.0 (2.5), and the dimmed continuation is 1.0 (1.5). The observable benefit is less visually heavy curves while retaining their emphasis and navigability.

The existing pointer-navigation integration test was strengthened before production changes and failed on the rendered 2.0-point score curve. It now checks visible stroke widths in both charts, clicks highlighted and dimmed assessments to select the correct positions, and checks the full played-game curve after navigation. Independently restoring each of the four former stroke widths makes the test fail; all mutations were restored. A synthetic GPU preview was rendered and inspected with `KATASTRO_CHART_PREVIEW=local/thinner-charts.png cargo test --test ui variation_charts_dim_only_the_original_future_and_highlight_the_saved_continuation` (requires a working wgpu adapter).

Formatting, strict workspace Clippy and the full workspace suite pass (112 tests; 14 opt-in checks excluded). The macOS bundle was rebuilt and passed strict code-sign verification. Real-engine and live-login checks were not rerun for this presentation change.

## Library training with a 1-dan opponent (2026-11-13)

The application now finds lost leads in the synced library, runs a blind five- or ten-move attempt, reports reductions in cumulative losses on the player’s own moves against the recorded continuation over the same ply window, and offers retry or reveal. Revealing produces up to three plausible legal lines, selectable through explicit buttons in the normal reviewer. Originals and their main line remain intact. [The training policy](training.md) documents thresholds, model calibration, storage, cancellation, and current limitations.

Public `Trainer` integration tests use real SGF parsing and temporary review/cache databases. Their initial pre-implementation run found no playable exercise, no opponent request, and no grade. The real-engine test initially rejected `rank_1d` because no HumanSL model was loaded. The worker test initially could not prepare a session. Real UI tests first failed because the training entry and Start controls were absent; a later line-selection test failed because no exploration button existed. Cache-budget reuse, reopening completed exercises, player-scoped progress, and clearing training evaluations each had an observed behavioral failure before their fixes.

The tests cover both player colors, hidden future moves/comments, exact-budget cache reuse and model identity, blind retries, matching-ply grading after two passes, strict rejection of an attempt rescued by an opponent blunder, proportional HumanSL sampling and illegal-move filtering, canceled/partial replies, legal plausible-line filtering, unchanged original SGFs, persistent review exploration, and completed progress surviving cache eviction. Controlled subprocess tests return screening results in reverse order and cancel a pending opponent. Local HTTP fixtures verify atomic model installation, offline reuse, and rejection of a corrupted download. Real pointer input plays an attempt, retries, gives up, selects a suggested line, and returns to the exercise position.

Thirteen initial temporary mutations produced behavioral test failures: bypassing confirmation, using Black's perspective for White, exposing the original document instead of its prefix, ignoring the strict mistake guard, always sampling the first policy move, admitting weak lines, ignoring the completed-exercise opt-in, dropping board input, bypassing download integrity, sharing progress across player names, disabling cache eviction, omitting the HumanSL model, and dropping saved exploratory branches when repeating an exercise. All were restored. Mutation logs and the synthetic GPU previews are in ignored `local/`; screenshots supplement outcome tests.

The explicit real KataGo/HumanSL compatibility check passed: rank-1d policy probabilities and normal 512-visit multi-move analysis share one persistent process. A separate complete real-library check found an exercise among the user's recent local records, played and graded an attempt, revealed legal lines, and verified every input SGF hash was unchanged. It read the configured account name without accessing login credentials and used temporary training databases. The final cumulative-grading functional run took about 244 seconds on this machine; it is not a performance comparison. Both real checks and the optional GPU-preview invocation have documented prerequisites in `training.md`.

Final verification passes: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (133 passed; 16 opt-in checks excluded), and `cargo check --no-default-features --lib`. The real HumanSL protocol check also passes after restoring its mutation. The optimized macOS bundle was rebuilt and passed `codesign --verify --deep --strict local/Katastro.app`. The official opponent model was verified and installed in the application's model directory; the running app was not restarted.


The final grading policy uses the sum of negative changes on the player's own moves, superseding an earlier endpoint comparison. Two regression cases first failed behaviorally: clean cumulative improvement was rejected when the recorded opponent had restored the lead, while five small mistakes were incorrectly accepted when AI replies gave the losses back. Both now produce the intended grades. Tests also assert exact own-move totals with an opponent that worsens the position, ensure later player gains cannot cancel earlier losses, accumulate winrate and score independently, and exclude recorded mistakes beyond an early two-pass finish. The major-mistake test has an improving cumulative total and explicitly distinguishes the strict setting from its opt-out.

Additional temporary mutations include opponent-caused drops, use signed changes instead of nonnegative loss independently for each metric, count only the first player move, and grade the full planned window after an early finish. The strict-guard mutation is checked again against the strengthened cumulative-loss case. The UI report names the two loss totals and the reduction, rather than reporting an endpoint gain.

## Fluent training turns (2026-11-13)

The board retains its brightness and position while the opponent thinks, with text status instead of a per-move spinner. The next turn opens on the correlated HumanSL reply, independently of pending grading searches. During the player's turn, grading uses at most seven of eight analysis slots so the next opponent request can start; searching and final grading retain all eight. The report still waits for complete before/after evaluations and uses the same cumulative own-move loss criteria.

Before implementation, a public `Trainer` integration test with real SGF parsing and temporary databases failed because the phase stayed `Opponent` after an AI reply with grading held. A real pointer-input test failed because the rendered board became half transparent. The passing tests now complete all five turns with evaluations withheld, exercise an accumulated grading backlog, return results out of order, and verify the final loss totals. Pointer input checks unchanged board bounds/color, no opponent-turn spinner, blocked extra clicks, and a playable next turn. A controlled engine subprocess independently verifies that the second move reaches HumanSL before the first move's evaluation completes, then cancels the pending opponent.

Five temporary mutations were detected and restored: reinstating the grading gate, removing the reserved slot, fading the board, restoring the per-move spinner, and allowing extra clicks. Formatting, strict workspace Clippy, the full workspace suite (135 passed, 16 opt-in checks excluded), and a build of the library without GUI features pass.

The complete real-library flow passed with KataGo 1.18.2 (Metal), the normal b10c384 model and the official b18c384 HumanSL model: five moves, a complete report, legal revealed lines, and unchanged source SGF hashes. A separate before/after measurement compared the committed `Trainer` at `5132298` with this implementation using the same five recent 19×19 games and the same screening-only cache (no grading results). Review analysis was paused and the two runs were sequential. The four non-final turns produced these worker-snapshot timings:

| Metric | Before | After |
| --- | ---: | ---: |
| Median player move → next playable turn | 10.95 s | 1.80 s |
| Median wait after the AI reply | 10.38 s | 0 ms |
| Final reply → completed report | 49.11 s | 81.29 s |

The final row is one observation per version. Automated input plays immediately, so unfinished grading accumulates at the end; the change removes that work from the per-move interaction, without reducing evaluation budgets. The complete seeded checks took 112.09 s and 108.11 s respectively. These are single stochastic attempts, not a claim of faster engine throughput or displayed-frame latency. An earlier run overlapped review analysis and is excluded from the comparison. Logs, the cache seed, and mutation evidence remain under ignored `local/`. The timing invocation and optional seed are documented in `training.md`.

The synthetic GPU preview was rendered and inspected using the documented `KATASTRO_TRAINING_PREVIEW` invocation; `thinking.png` shows the unchanged board brightness without a spinner. The optimized macOS bundle was rebuilt and passed strict code-sign verification.
