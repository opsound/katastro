# Analysis scheduling benchmark

`tests/utilization.rs` measures completed work and selection latency through the public review worker with real KataGo. It requires an installed engine, a compatible model, and a private directory containing at least two legal 19×19 SGFs with 150–350 main-line positions. It selects two games deterministically by length and content hash and uses their first 96 moves. Originals are read only. Temporary SGFs, review databases and analysis caches are removed after each run; results contain hashes and timings, not game content or player names.

The workload first warms a persistent engine on a separate synthetic game, then opens an uncached real-game prefix. It measures complete initial chart coverage and all 97 positions reaching 64 visits. It pauses and checks that completed results were persisted. Next it creates 24 legal independent variations in middle-game positions, warms another engine, opens the saved review, and makes eight selections while background analysis continues. It records the first estimate and 64-visit result for each selected position, and elapsed time until all 24 variations reach 64 visits, again checking the cache. Selection timings end at worker snapshots; they exclude GUI frame presentation. Visits measure effort, not evaluation accuracy.

```sh
KATASTRO_TEST_ENGINE=/absolute/path/to/katago \
KATASTRO_TEST_MODEL=/absolute/path/to/model.bin.gz \
KATASTRO_SGF_DIR=/absolute/path/to/private/sgfs \
KATASTRO_UTIL_RECORD=local/utilization.json \
cargo test --release --test utilization -- --ignored --nocapture
```

The default is three trials on each of two games. Set `KATASTRO_UTIL_TRIAL=0` and `KATASTRO_UTIL_SAMPLE=0` to run one case. The output records the engine/model/configuration profile, source hashes, position count and individual timings. Keep outputs in ignored `local/`.

For before/after comparisons, copy the same harness into a detached baseline worktree and build optimized binaries ahead of measurement. **Use separate Cargo target directories for the two worktrees**: sharing a target directory can reuse stale artifacts when switching checkouts. Verify that the binaries differ, while the engine/model/configuration profiles and source hashes match. Alternate execution order across cases. Run only one engine workload at a time, with no compilation or other benchmark in parallel. Report elapsed times and their variability rather than inferring throughput from request counts. Machine load, thermals, model, board size and engine backend affect results.

On macOS, `ioreg -r -c AGXAccelerator -a` can expose `PerformanceStatistics` → `Device Utilization %`. This is a coarse system-wide counter, including other applications and window rendering; it is supplementary evidence, not proof that Katastro saturates the GPU. A busier device is useful only when completed analysis and interaction behavior also improve.

Real-engine protocol and cache regression checks are separate from this workload:

```sh
KATASTRO_TEST_ENGINE=/absolute/path/to/katago \
KATASTRO_TEST_MODEL=/absolute/path/to/model.bin.gz \
cargo test --test engine -- --ignored --nocapture --test-threads=1

KATASTRO_TEST_ENGINE=/absolute/path/to/katago \
KATASTRO_TEST_MODEL=/absolute/path/to/model.bin.gz \
cargo test --test fox live_fox_analysis_and_cache -- --ignored --nocapture
```

## Resuming a fully cached review

The release benchmark seeds a temporary cache with 64-visit evaluations and ownership for every node of one SGF. With the review capped at 64 visits, it measures the time from each Analyze command to the worker’s running snapshot after a pause. Each of seven resumes checks that the complete chart is available. The engine process and file-system cache are warm. No search or frame presentation is timed.

```sh
KATASTRO_TEST_ENGINE=/absolute/path/to/katago \
KATASTRO_TEST_MODEL=/absolute/path/to/model.bin.gz \
KATASTRO_TEST_SGF=/absolute/path/to/game.sgf \
cargo test --release --test performance warm_resume_to_worker_snapshot -- --ignored --nocapture
```

On Apple Silicon, replacing software SHA-256 with the accelerated implementation already used by the TLS stack reduced the following median times. Both builds hashed the complete model and executable on every resume. Each pair used the same local SGF and assets, with execution order alternated across games.

| Main-line positions | Previous median | Updated median |
| ---: | ---: | ---: |
| 199 | 112.88 ms | 24.52 ms |
| 217 | 113.41 ms | 25.23 ms |
| 229 | 113.17 ms | 25.67 ms |
