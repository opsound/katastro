# Training from the game library

Open **Train**, or **Games → Train from these games**. The player name defaults to the synced account and is matched case-insensitively against SGF `PB`/`PW`. Use the latest 20 library games by default; increase the limit in setup. Choose five or ten of your moves per attempt. Game order is randomized when starting and choosing Next; ready candidates across that selection take precedence over starting a new discovery pass. Passed and given-up opportunities, including repeated misses of the same key move, are skipped unless **Include completed exercises** is checked. Failed attempts remain eligible.

The application pauses review analysis and background discovery while training. All SGF reads, SQLite access, model verification/downloads, and engine work run on worker threads. The training queue follows Engine settings → Parallel positions (eight by default), and each search uses the configured threads per position. The review visit limit does not alter training budgets. During the player's turn, one fewer grading request than that capacity can run, leaving a slot for the next HumanSL reply; opponent requests go to the front of the queue. Newly played positions take priority over queued recorded-line preparation. Searching and final grading can use all configured slots. With one slot, background grading waits during the player’s turn. Retry, Next, and exploration cancel outstanding searches before submitting work for the next stage. Going back cancels the session; the UI does not wait for a network request to finish. Leaving an unfinished attempt does not mark the exercise complete.

## Finding turning points

With no game open, the review worker prepares the synced library using the normal model and its existing engine process; HumanSL is not loaded for discovery. **Pause preparation** stops this work, and **Prepare training** resumes it. Opening a game or entering training cancels pending discovery requests. Compatible completed results remain reusable; late canceled replies cannot become results for another preparation session.

Review refinement has no practical final endpoint. After a complete, durably cached main-line pass at 512 visits or more (currently the 1024-visit tier), candidates are indexed directly from those evaluations without extra engine requests. The index refreshes after later complete fourfold-deeper passes. Unfinished streamed estimates never trigger indexing. Review interaction keeps priority; new engine work for unreviewed library games runs only with no game open.

Search original main-line moves made by the named player. Reuse compatible cached analysis at 64 visits or deeper for screening: a loss of at least 3 points, giving the coarse pass a wider net. Confirm candidates at a requested 512 visits, or reuse the latest compatible review results at 512 visits or deeper during background preparation. A confirmed exercise requires a loss of **at least 5 points on one of your moves**, whether you were ahead or behind. Winrate does not affect selection, grading, suggested-line filtering, or training reports. Values are converted from the engine's fixed Black perspective to the player's perspective.

Candidates in each game are considered from largest screened point loss downward. The selected games are shuffled, so one game with many mistakes does not dominate selection. Ready games still take priority over games needing analysis. The recorded continuation must contain enough moves for the selected attempt length. Unreadable records, unknown players, and unsupported positions are skipped. This bounded, screened search can miss turning points that only become clear with deeper analysis; it is not an exhaustive tactical proof.

Repeated misses of the same top searched move in one game are grouped as one opportunity. This groups the player's missed chances even when the opponent also overlooks that move. Playing at the key intersection starts a new opportunity, so a later capture or ko can produce a distinct problem there. This is a concrete move-based heuristic, not a claim that all similar tactical concepts are recognized. Occurrences with shorter remaining continuations stay in the index so the chosen attempt length can use an earlier occurrence of the same opportunity.

Confirmed candidates become available immediately, even while the rest of the library is still being prepared. A completed scan also records games with no qualifying positions. Starting training checks these database records first, so a newer unprepared game cannot hold up an older ready exercise in the selected library. If nothing suitable is ready, foreground discovery remains available.

After scanning a game during idle preparation, the worker prepares one eligible occurrence per opportunity, caching its recorded comparison window at the grading budget: five player moves plus replies, or the remaining game when shorter. Already indexed games also receive this preparation, so older catalogs benefit. Indexing an active review stays limited to cached results. Opening an exercise immediately queues any missing recorded-window evaluations while showing the playable board; choosing ten moves extends the default prepared window. Discovery's deeper review values remain separate from the exact-budget grading cache, and normal review results are never downgraded.

The attempt receives only the position's ancestry and moves made during the attempt. Future original moves, comments, annotations, results, evaluations, charts, navigation controls, and suggestions are not exposed by the training screen. Illegal moves leave the attempt unchanged. The board keeps its normal brightness while the opponent thinks, with a quiet text status beside it. Extra moves are blocked until the reply arrives, then play resumes immediately while evaluations continue in the background. There is no per-move spinner. Final grading waits for the player's before/after evaluations on both lines, without waiting for chart-only endpoints or preparation beyond an early finish.

## Opponent and grading

KataGo 1.15 or newer loads the normal analysis model and the official `b18c384nbt-humanv0.bin.gz` HumanSL model in one persistent process. Opponent requests use `humanSLProfile=rank_1d`, one visit, and `includePolicy=true`. Moves are sampled proportionally from `humanPolicy` at full temperature, filtered through the application's legality checks. This follows KataGo's [Human SL Analysis Guide](https://github.com/lightvector/KataGo/blob/master/docs/Analysis_Engine.md#human-sl-analysis-guide). It approximates human 1-dan play; it is not a certified Pandanet rank. Normal search visits are not used as a substitute for rank calibration.

The 99,066,230-byte model comes from the [official v1.15.0 release](https://github.com/lightvector/KataGo/releases/tag/v1.15.0). A streaming download goes to a temporary file, then a pinned SHA-256 check precedes atomic installation. The installed file is verified before reuse. Interrupted or corrupt downloads never replace the installed model. No game content is sent to the model host.

The normal model, without a HumanSL override, assesses all grading positions at a requested **512 visits**. Both lines cover the same number of plies. For **each of the player's own moves**, calculate the change between its immediate before/after evaluations, from that player's perspective:

```text
score loss   = sum(max(0, score before - score after))
improvement  = recorded loss - attempt loss
```

Opponent moves are excluded. A later improvement, even on the player's own move, never cancels an earlier loss. For example, an original move losing 8 points still contributes 8 to the baseline when the opponent immediately gives them back. Smaller mistakes also accumulate: five 2-point losses count as 10 points, regardless of opponent gifts between them. The 5-point threshold selects exercises and flags major mistakes; losses below it still count toward grading.

Two consecutive passes end an attempt early and shorten the recorded window accordingly. Only the player's moves inside that window count. The recorded window is requested before play, reusing compatible cached results. Attempt evaluations are queued as moves arrive, including the position after the final AI reply. That final opponent position completes the chart but never enters the grade.

A pass requires:

- A **reduction in cumulative own-move point loss** of at least **3 points**.
- With the default strict setting, none of the player's own moves loses **5 points or more** between its before/after evaluations. An overall reduction does not excuse another major mistake.

These are explicit training thresholds, not statistical confidence intervals or proof of understanding. Evaluations remain estimates, and opponent replies are stochastic. The report shows recorded and attempted loss totals, the reduction in points, and whether a major mistake occurred. A failed attempt offers **Retry without hints**; success or **Give up and explore** unlocks the review.

The report includes a score graph with **Recorded** and **Attempt** overlaid from the exercise position through the final reply, aligned by moves from the start and shown from the player's perspective. Both players' moves appear in the graph; only the player's losses affect the pass/fail result. Pass/fail and the report controls are available as soon as grading is complete. Any remaining endpoints appear later under **Finishing comparison…**, on a fixed move axis; missing values are omitted, never plotted as zero. Completion updates the same saved attempt without changing its grade. Leaving the report cancels remaining work, so its saved curves can remain partial. The chart area is resizable and stays hidden during an attempt and after blind retry.

**Next problem** is available after pass, fail, and while exploring. It skips the whole opportunity for this session, including when completed exercises are enabled, and resets the board, report, and reveal state. Retry still repeats the current position without hints. Skipping a failed problem does not mark it passed or given up; it remains eligible in a later session. If the selection is exhausted, setup explains that no further qualifying position is available.

## Review and persistence

A fresh normal-model search returns principal variations up to 12 plies long. Keep up to three legal multi-move lines with at least 16 visits, within 2 score points of the most-visited candidate from the player's perspective. Do not pad the list with clearly weak moves when fewer lines qualify.

A private review SGF contains the original game, saved attempts, and suggested branches. **Explore line 1/2/3** selects the end of that continuation; the regular tree, board, charts, and navigation support further exploration. **Exercise position** returns to the start. The original main line stays on top, and source SGFs are never rewritten. Normal review persistence saves further user variations. Repeating an exercise restores those saved branches before adding new attempts and lines.

During exploration, stones played after the exercise position are numbered **1, 2, 3…**. The numbers follow the selected branch, including moves you add yourself; earlier stones remain unnumbered. Passes consume a number and captured stones disappear, so visible numbers can have gaps. Returning to ordinary review or opening another game clears the labels. These display labels do not modify the SGF.

Progress is scoped to the named player and training-policy version. The score-only policy uses a new catalog and progress identity: old winrate-selected indexes and completions cannot suppress or qualify its exercises. Existing attempts and generated reviews remain stored; compatible position evaluations are reused. Ordinary review charts retain both score and winrate. Completed attempts, their reports, and their branches live in `reviews.sqlite`, independently of the evictable analysis cache. Evaluations are keyed by full position history, engine/model/settings identity, grading semantics, and requested budget. Actual visits are stored and validated; deeper normal cached results are never downgraded. Exact-budget training results are reusable even if the engine's actual visit count slightly exceeds its requested budget.

The derived candidate catalog lives in `analysis.sqlite` alongside evaluations. Its identity includes semantic SGF content, the frozen main line, player name and color, engine/model/settings identity, and discovery-policy version. Changed games or profiles cannot reuse an incompatible catalog. The grouped-candidate catalog has its own version; existing score-only attempt identities and grades remain valid. Completion groups are stored with attempts and recovered for older completed positions before offering other occurrences. Old root-only or policy-only caches receive a bounded 512-visit search for missing key-move metadata during idle or foreground discovery. The completed-review hook remains cache-only and defers incomplete metadata, including any affected legacy completions. Clearing analysis also clears discovery results, including negative scans, without deleting attempts or review branches.

The app's data directory holds `models/b18c384nbt-humanv0.bin.gz` and generated `Training/*.sgf` reviews. None belong in Git. Active attempts are currently held in memory; reopening the app starts a new attempt. Session setup choices are not yet persisted.

## Explicit integration checks

Routine tests require no account, model, GPU, or network service outside local loopback fixtures:

```sh
cargo test --test training --test training_score --test training_grading --test training_catalog --test training_selection --test training_background --test training_worker --test training_ui --test pandanet_ui
cargo test --test numbered_variations
cargo test --lib explored_lines_number_stones
```

The desktop exploration test clicks the actual line, navigation, and board controls through the review worker. To also render its frame locally (requires a working GPU backend):

```sh
KATASTRO_NUMBERED_PREVIEW=local/numbered-exploration.png cargo test --lib explored_lines_number_stones
```

Real model compatibility (requires KataGo >=1.15 and both downloaded models):

```sh
KATASTRO_TEST_ENGINE=/path/to/katago \
KATASTRO_TEST_MODEL=/path/to/normal.bin.gz \
KATASTRO_TEST_HUMAN_MODEL=/path/to/b18c384nbt-humanv0.bin.gz \
cargo test --test engine real_human_policy -- --ignored
```

The complete real-library check additionally requires permission to read the user's local Pandanet library. It reads the configured account name, never a credential; it uses temporary training databases and verifies original-file hashes. It may take several minutes and requires a qualifying position among the five newest records:

```sh
KATASTRO_TRAINING_LIBRARY=/path/to/Pandanet \
KATASTRO_TEST_ENGINE=/path/to/katago \
KATASTRO_TEST_MODEL=/path/to/normal.bin.gz \
KATASTRO_TEST_HUMAN_MODEL=/path/to/b18c384nbt-humanv0.bin.gz \
cargo test --test training_worker real_library_exercise -- --ignored --nocapture
```

This check also reports elapsed time from each player move to the AI reply and to the next playable turn (or final report), including any wait after the reply. These are worker-snapshot timings; there are no timing thresholds in routine tests. Compare the same corpus, models and machine with other engine work paused. Human replies are sampled, so separate runs need not follow the same positions.

For repeated latency comparisons, `KATASTRO_TRAINING_SCREENING_CACHE` can point to a private copy of an analysis cache containing only results below 512 visits. The check copies it into its temporary directory and rejects any retained grading results. Use the same seed in both versions to avoid repeating the full screening pass while still performing confirmation and grading. Never edit the application's live cache to prepare a benchmark seed.

The background-preparation check uses only the normal model. It measures time to the first ready exercise from an empty temporary cache, then measures opening that candidate in a fresh application-layer session without a discovery request. It also verifies source hashes and a legal first training move. This is preparation versus cached-start latency, not engine throughput or displayed-frame latency:

```sh
KATASTRO_TRAINING_LIBRARY=/path/to/Pandanet \
KATASTRO_TEST_ENGINE=/path/to/katago \
KATASTRO_TEST_MODEL=/path/to/normal.bin.gz \
cargo test --test training_preparation_real -- --ignored --nocapture
```

The grading benchmark freezes one real exercise and its first sampled HumanSL continuation in a private directory. Later runs replay those same replies while still issuing real HumanSL requests; both the recorded and attempted positions therefore match across versions. It compares an indexed candidate with and without completed background preparation, reporting preparation cost separately from the wait after the final AI reply. Each attempt starts a fresh engine, reusing only the persisted analysis from preparation. Inputs stay unchanged, and no routine test asserts wall-clock thresholds. Run old and new binaries sequentially with other KataGo work paused:

```sh
KATASTRO_GRADING_BENCH_ROOT="$PWD/local/grading-benchmark" \
KATASTRO_GRADING_RESULTS="$PWD/local/grading-results.json" \
KATASTRO_TRAINING_LIBRARY=/path/to/Pandanet \
KATASTRO_TEST_ENGINE=/path/to/katago \
KATASTRO_TEST_MODEL=/path/to/normal.bin.gz \
KATASTRO_TEST_HUMAN_MODEL=/path/to/b18c384nbt-humanv0.bin.gz \
cargo test --release --test training_grading_latency -- --ignored --nocapture
```

The optional screening-cache seed above can also speed up initial benchmark fixture creation. Keep the generated exercise, source paths, opponent line, and SQLite seed private and use the same directory for both versions. The seed restricts catalog scope to the first confirmed candidate without fabricating evaluations or visits. Compare `position_sha256` in the results before interpreting timing differences.

Optional synthetic GPU previews, including `report.png`, require a working wgpu adapter and an existing output directory:

```sh
mkdir -p local/training-preview
KATASTRO_TRAINING_PREVIEW="$PWD/local/training-preview" cargo test --test training_ui
```
