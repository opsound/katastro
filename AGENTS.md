# Project instructions

## Product requirements

Build a native macOS Go review application in Rust. Compiled Rust with GPU-rendered controls is acceptable. Keep the original SGF main line on the top row of a tree that grows from left to right. User moves create persistent variations without replacing the played game. Analysis must be reusable across reopen and progressively refined after a fast initial chart pass.

The framework research and architecture are documented in `docs/research.md` and `docs/design.md`. Completed MVP behavior and test evidence are recorded in `docs/validation.md`; architectural proposals alone are not evidence of completed features.

## Mandatory test driven development

For every new feature or behavior change:

1. Describe the observable user benefit and a plausible regression that would remove it.
2. Write an integration test through the public application API or real UI interaction before implementing production behavior.
3. Run it and observe a failure caused by the missing behavior. A compile error, missing dependency, or broken fixture does not establish the behavioral failure; add only the minimum interface needed to reach it.
4. Implement the smallest change that makes the test pass.
5. Refactor while retaining the passing test, then run the relevant regression checks.
6. Check discriminating power by temporarily removing or breaking the claimed behavior and confirming the test fails. Restore the implementation. Record this evidence in the change description; targeted mutation tools are also acceptable.

Favor integration tests over unit tests. Use real SGF parsing, real temporary SQLite databases, and application commands together. Use a controlled fake engine subprocess when testing asynchronous protocol behavior. Keep a small separate suite against real KataGo; fake-engine tests alone do not validate protocol compatibility.

Tests must assert outcomes: persisted branches, cache reuse without a duplicate request, chart coverage before refinement, or correct board selection after input. Construction tests, getters, screenshots alone, or assertions that copy implementation details are insufficient feature evidence. Focused unit tests may supplement integration tests for difficult rule or layout edge cases.

Use explicit synchronization and a controllable clock for scheduling tests. Avoid arbitrary sleeps and GPU-dependent timing assertions in routine tests. Performance work requires representative SGFs, a baseline, a useful metric, and measured comparison; do not claim a speedup from request-count tests alone.

Every feature needs corroborating tests even when the edit is small or reversible. Do not defer test coverage, silently skip required tests, or replace a missing feature with a passing placeholder. Report what was tested and any environmental limitations honestly.

Once Cargo manifests exist, standard checks are `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`. Use the affected integration test during the red and green cycle and run the full required checks before delivery. Real-engine and GPU tests must have documented explicit invocations and prerequisites.

## Architecture and data

- Keep SGF, Go rules, the game tree, analysis scheduling, and persistence independent of the GUI framework.
- Never run engine I/O, model loading, database I/O, or deep analysis on the UI thread.
- Use a persistent KataGo `analysis` process with newline-delimited JSON. Correlate responses with unique request IDs, document generation, and node identity; replies may be out of order or arrive after cancellation.
- Cache identity must include full relevant position history, board size, setup stones, side to move, komi, rules, model identity, and analysis semantics. A filename or board-stone hash is insufficient.
- Store actual result visits; a lower-visit or stale result must not replace a deeper compatible result. More visits indicate search effort, not guaranteed accuracy.
- Fix evaluation perspective across the chart. Treat missing analysis separately from zero points or zero winrate.
- Persist review variations as user data, separately from the evictable analysis cache. Reopening the source SGF must recover recorded variations.
- Freeze the imported main line explicitly. Selection, stronger analysis, branch insertion, and child sorting must not change it.

## References and Git

Use `~/KataGo` and `~/katrain` as read-only references unless a separate task requests changes. The user authorized reading SGFs in `~/Downloads` for local checks. Keep originals unchanged and private game content out of Git; use small synthetic or sanitized fixtures for portable tests.

Use Git for this project. Keep changes reviewable and include test evidence in commit or PR descriptions. Do not invent a remote, change global Git settings, or commit engine binaries, model weights, analysis databases, or personal game files.
