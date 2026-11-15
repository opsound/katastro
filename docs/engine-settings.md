# Engine settings

Open **Engine** in the review toolbar. The executable and neural-network model selectors remain at the top. Analysis settings are saved locally and restored after restarting the app. **Save engine settings** pauses review analysis; choose **Analyze** to resume with the new settings. **Restore analysis defaults** changes only the analysis controls, keeping the selected paths; save to apply it.

| Control | Default | Range | Effect |
| --- | --- | --- | --- |
| Limit review analysis / Visits per position | Unlimited | 64–1,048,576 when enabled | Stop review refinement when every supported position reaches the requested effort. Enabling the limit initially selects 1,024 visits. |
| Parallel positions | 8 | 1–32 | Number of positions KataGo can search simultaneously. Review refinement, idle library preparation, and training use this capacity. |
| Threads per position | 1 | 1–16 | Search threads assigned to each position. Total possible search threads are the product of this and parallel positions. |

A visit measures search effort, not guaranteed accuracy. The chart still gets a fast one-visit pass before refinement through 8, 64, and fourfold increases. A limit between those tiers, such as 100, becomes the final tier. Selecting a position or adding a variation respects the same limit. Already deeper cached results remain visible; a missing ownership map can still receive its bounded 64-visit request. Raise the limit to continue from compatible saved work. Actual engine visits can exceed the requested budget slightly, and the actual count is retained.

Parallel positions favors working through the game; more threads per position assigns additional resources to each individual search. Higher values are not necessarily faster: the useful balance depends on the hardware and workload. Katastro sets the GPU batch capacity to the product of the two controls for its single GPU server. These controls correspond to `numAnalysisThreads`, `numSearchThreadsPerAnalysisThread`, and `nnMaxBatchSize` in [KataGo's analysis configuration](https://github.com/lightvector/KataGo/blob/v1.18.2/cpp/configs/analysis_example.cfg#L80-L131). The defaults retain the previous configuration; no performance improvement is claimed for other combinations.

Training shares the compute controls but retains its fixed screening, confirmation, grading, and opponent budgets. The review limit cannot weaken an exercise's grade. During the player's turn, one slot is reserved for the next opponent reply. With one parallel position, background grading therefore waits while the player is choosing a move and finishes after play. Changing effort or concurrency keeps the same compatible analysis identity; model, engine, game history, rules, komi, perspective, and evaluation semantics still partition the cache. Legacy saved settings inherit the existing unlimited / 8 / 1 defaults.

All persistence, engine configuration, and scheduling changes run on workers. Out-of-range application commands are rejected before stopping analysis or replacing saved settings.

## Verification

Routine integration tests use synthetic SGFs, temporary SQLite databases, actual settings-pane input, and controlled engine subprocesses:

```sh
cargo test --test engine_settings --test engine_settings_ui --test scheduler --test analysis_queue --test training_background --test training_worker
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --no-default-features --lib
```

A real KataGo check exercises two parallel positions, two threads per position, and a 100-visit review limit. It requires a local KataGo executable and normal model, plus a working backend for that binary:

```sh
KATASTRO_TEST_ENGINE=/path/to/katago \
KATASTRO_TEST_MODEL=/path/to/normal.bin.gz \
cargo test --test engine live_custom_analysis_settings -- --ignored
```

The existing `real_human_policy` check in `tests/engine.rs` also exercises HumanSL and legal suggested continuations. Its invocation and model prerequisites are in [training.md](training.md#explicit-integration-checks).

Optional rendering of the settings pane requires a working GPU backend and an existing output directory:

```sh
KATASTRO_ENGINE_SETTINGS_PREVIEW="$PWD/local/engine-settings.png" \
cargo test --test engine_settings_ui
```
