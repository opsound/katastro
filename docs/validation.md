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
