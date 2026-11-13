# Training from the game library

Open **Train**, or **Games → Train from these games**. The player name defaults to the synced account and is matched case-insensitively against SGF `PB`/`PW`. Search the latest 20 library games by default; increase the limit in setup. Choose five or ten of your moves per attempt. Passed and given-up exercises are skipped unless **Include completed exercises** is checked. Failed attempts remain eligible.

The application pauses review analysis while training. All SGF reads, SQLite access, model verification/downloads, and engine work run on the training worker. Its queue keeps up to eight independent positions outstanding. During the player's turn, at most seven grading requests run, leaving a slot for the next HumanSL reply; opponent requests go to the front of the queue. Searching and final grading can use all eight slots. Going back cancels the session; the UI does not wait for a network request to finish. Leaving an unfinished attempt does not mark the exercise complete.

## Finding turning points

Search original main-line moves made by the named player. Reuse compatible cached analysis at 64 visits or deeper for screening: at least 60% winrate before the move and a drop of at least 10 percentage points. Confirm candidates at a requested 512 visits. A confirmed exercise requires **at least 65% winrate, a positive score, and a drop of at least 15 percentage points**. Values are converted from the engine's fixed Black perspective to the player's perspective.

Candidates in each game are considered from largest screened winrate loss downward, with newer games searched first. The recorded continuation must contain enough moves for the selected attempt length. Unreadable records, unknown players, and unsupported positions are skipped. This bounded, screened search can miss turning points that only become clear with deeper analysis; it is not an exhaustive tactical proof.

The attempt receives only the position's ancestry and moves made during the attempt. Future original moves, comments, annotations, results, evaluations, charts, navigation controls, and suggestions are not exposed by the training screen. Illegal moves leave the attempt unchanged. The board keeps its normal brightness while the opponent thinks, with a quiet text status beside it. Extra moves are blocked until the reply arrives, then play resumes immediately while evaluations continue in the background. There is no per-move spinner. Final grading waits for every required evaluation before displaying a report.

## Opponent and grading

KataGo 1.15 or newer loads the normal analysis model and the official `b18c384nbt-humanv0.bin.gz` HumanSL model in one persistent process. Opponent requests use `humanSLProfile=rank_1d`, one visit, and `includePolicy=true`. Moves are sampled proportionally from `humanPolicy` at full temperature, filtered through the application's legality checks. This follows KataGo's [Human SL Analysis Guide](https://github.com/lightvector/KataGo/blob/master/docs/Analysis_Engine.md#human-sl-analysis-guide). It approximates human 1-dan play; it is not a certified Pandanet rank. Normal search visits are not used as a substitute for rank calibration.

The 99,066,230-byte model comes from the [official v1.15.0 release](https://github.com/lightvector/KataGo/releases/tag/v1.15.0). A streaming download goes to a temporary file, then a pinned SHA-256 check precedes atomic installation. The installed file is verified before reuse. Interrupted or corrupt downloads never replace the installed model. No game content is sent to the model host.

The normal model, without a HumanSL override, assesses all grading positions at a requested **512 visits**. Both lines cover the same number of plies. For **each of the player's own moves**, calculate the change between its immediate before/after evaluations, from that player's perspective:

```text
winrate loss = sum(max(0, winrate before - winrate after))
score loss   = sum(max(0, score before - score after))
improvement  = recorded loss - attempt loss
```

Accumulate the two metrics independently. Opponent moves are excluded. A later improvement, even on the player's own move, never cancels an earlier loss. For example, an original move losing 30 percentage points still contributes 30 to the baseline when the opponent immediately gives them back. Several smaller mistakes also accumulate; five losses of 10 points of winrate count as 50, regardless of opponent gifts between them. These sums are losses in percentage points, not a final win probability, and can exceed 100.

Two consecutive passes end an attempt early and shorten the recorded window accordingly. Only the player's moves inside that window count. The final opponent reply is played but needs no evaluation for grading. Before/after evaluations around the recorded player's moves are requested in parallel when grading begins, reusing compatible cached results.

A pass requires:

- A **reduction in cumulative own-move loss** of at least **10 percentage points** of winrate, or **3 points** of score.
- Neither cumulative loss is worse than the recorded line by more than **2 percentage points** or **1 point**, respectively.
- With the default strict setting, none of the player's own moves loses **15 percentage points** or **3 points** between its before/after evaluations. An overall reduction does not excuse another major mistake.

These are explicit training thresholds, not statistical confidence intervals or proof of understanding. Evaluations remain estimates, and opponent replies are stochastic. The report shows recorded and attempted loss totals, the reduction in each metric, and whether a major mistake occurred. A failed attempt offers **Retry without hints**; success or **Give up and explore** unlocks the review.

## Review and persistence

A fresh normal-model search returns principal variations up to 12 plies long. Keep up to three legal multi-move lines with at least 16 visits, within 5 percentage points and 2 points of the most-visited candidate from the player's perspective. Do not pad the list with clearly weak moves when fewer lines qualify.

A private review SGF contains the original game, saved attempts, and suggested branches. **Explore line 1/2/3** selects the end of that continuation; the regular tree, board, charts, and navigation support further exploration. **Exercise position** returns to the start. The original main line stays on top, and source SGFs are never rewritten. Normal review persistence saves further user variations. Repeating an exercise restores those saved branches before adding new attempts and lines.

Progress is scoped to the named player. Completed attempts, their reports, and their branches live in `reviews.sqlite`, independently of the evictable analysis cache. Evaluations are keyed by full position history, engine/model/settings identity, grading semantics, and requested budget. Actual visits are stored and validated; deeper normal cached results are never downgraded. Exact-budget training results are reusable even if the engine's actual visit count slightly exceeds its requested budget.

The app's data directory holds `models/b18c384nbt-humanv0.bin.gz` and generated `Training/*.sgf` reviews. None belong in Git. Active attempts are currently held in memory; reopening the app starts a new attempt. Session setup choices are not yet persisted.

## Explicit integration checks

Routine tests require no account, model, GPU, or network service outside local loopback fixtures:

```sh
cargo test --test training --test training_worker --test training_ui --test pandanet_ui
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

Optional synthetic GPU previews require a working wgpu adapter and an existing output directory:

```sh
mkdir -p local/training-preview
KATASTRO_TRAINING_PREVIEW="$PWD/local/training-preview" cargo test --test training_ui
```
