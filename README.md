# Katastro

A native macOS Go review MVP in Rust, using egui/eframe, wgpu's Metal renderer, a persistent KataGo analysis process, and SQLite.

Open or drop an SGF to restore its review and cached chart immediately. Missing positions get a one-visit evaluation before continuous refinement to 8, 64, 256, 1024 visits and beyond. The right column shows the current target and the selected position's actual visits. Points and winrate always use Black's perspective; missing values remain gaps. In a variation, both charts keep the shared game prefix visible, dim the original continuation after the fork, and highlight the active branch in gold through its saved continuation. Click either curve to select its position; returning to the original game restores its full curve.

Click an empty board intersection to explore a legal variation, even during analysis. Variations autosave after each move and remain separate from the original game, which stays on the top row of the tree. Click a tree node to revisit it, or use Up/Down to switch variation rows at the same column. Navigation keeps the selected node in view. Export SGF saves all branches to another file; the source stays unchanged.

The board shows all returned AI candidates, with the best searched recommendation in blue, and a dotted hollow ring for the next recorded move in its stone color. Other candidate colors reflect point loss against the best recommendation, from the moving player's perspective: green within 0.5 points, blending through yellow at 1.5, orange at 3, and red at 6 or more. Alternatives below 25 visits are subdued. Unscored policy previews are gray, with a blue outline identifying the leading preview. The sidebar includes a color legend and a fixed-height, scrollable list of every candidate, including Pass.

All board markers use equal-size circles containing the signed point change from the current position, from the moving player's perspective: positive gains points and negative loses points. Colors compare alternatives; numbers compare against the current position. The played move uses its candidate estimate when available, otherwise the following position's evaluation; missing estimates show `--`. Colored stones beside player names identify Black and White.

## Run

```sh
cargo run --release -- ~/Downloads/example.sgf
```

Or build a launchable app:

```sh
bash scripts/package-macos.sh
open local/Katastro.app
```

The bundle is for local use and has an ad hoc signature. It does not bundle KataGo or a neural network. On this Mac, Katastro detects `/opt/homebrew/bin/katago` and `~/katrain/katrain/models/b10c384h6nbttflrs.bin.gz`. Choose other paths in **Engine settings**. Cached reviews remain usable when the engine is unavailable.

| Input | Action |
| --- | --- |
| Left / Right | Previous / next move |
| Up / Down | Nearest variation above / below at the same column |
| Home / End | Root / last original move |
| P or Pass | Play a pass |
| Space | Analyze / pause |
| Cmd+O / Cmd+S | Open / export SGF |

Reviews and engine settings live in `~/Library/Application Support/Katastro/reviews.sqlite`. Analysis lives separately in `~/Library/Caches/Katastro/analysis.sqlite`; deleting this cache does not delete variations. `KATASTRO_DATA_DIR` and `KATASTRO_CACHE_DIR` override these directories for isolated runs. Only completed results with at least 64 actual visits are persisted; each compatible position keeps its deepest result, including all returned ranked suggestions. Cheaper and unfinished streamed estimates remain in memory. Older cheap cache rows are removed on startup, while saved reviews and eligible deeper rows are preserved. Existing results that stored only five candidates remain usable and acquire the full list as they refine. Cache compatibility includes game history, rules, komi, setup, side to move, model and executable checksums, and engine settings.

## Development and validation

Every feature starts with a failing behavioral integration test. Tests use actual SGF parsing, temporary SQLite databases, a controllable subprocess, and real UI input. [AGENTS.md](AGENTS.md) requires counterfactual checks of test strength. [Validation evidence](docs/validation.md) records the red/green cycles, mutation checks, real-engine runs, and measured latency.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The standard suite has 53 integration tests. Real-engine, corpus, and performance checks require local assets and run explicitly:

```sh
export KATASTRO_TEST_ENGINE=/opt/homebrew/bin/katago
export KATASTRO_TEST_MODEL="$HOME/katrain/katrain/models/b10c384h6nbttflrs.bin.gz"
export KATASTRO_SGF_DIR="$HOME/Downloads"
cargo test --release --test engine -- --ignored --nocapture
cargo test --release --test corpus -- --ignored --nocapture
cargo test --release --test performance -- --ignored --nocapture
```

A separate regression test opens and cancels real macOS Open and Export sheets, verifies that the review survives, and detects event-loop crashes. It requires a graphical macOS session:

```sh
cargo test --test native_dialogs -- --ignored --nocapture
```

The MVP supports single-game, square SGFs from 2×2 through 19×19, captures, ko/superko, passes, handicap setup, imported branches, comments, and common text encodings. Missing rules default visibly to Chinese. Midgame setup edits remain viewable but their subsequent analysis is unavailable rather than inventing history. Refinement continues until paused. Configurable budgets, ownership maps, editing game properties, and SGF collections remain future work.

Read the [framework comparison and source findings](docs/research.md), [architecture and acceptance plan](docs/design.md), and [original engine-only measurements](docs/engine-probe.json).
