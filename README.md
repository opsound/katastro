# Katastro

A native macOS Go review MVP in Rust, using egui/eframe, wgpu's Metal renderer, a persistent KataGo analysis process, and SQLite.

Open or drop an SGF to restore its review and cached chart immediately. Missing positions get a one-visit evaluation before the game is refined to 8 and 64 visits. Points and winrate always use Black's perspective; missing values remain gaps. Click either chart to navigate the played game.

Click an empty board intersection to explore a legal variation, even during analysis. Variations autosave after each move and remain separate from the original game, which stays on the top row of the tree. Click a tree node to revisit it. Export SGF saves all branches to another file; the source stays unchanged.

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
| Home / End | Root / last original move |
| P or Pass | Play a pass |
| Space | Analyze / pause |
| Cmd+O / Cmd+S | Open / export SGF |

Reviews and engine settings live in `~/Library/Application Support/Katastro/reviews.sqlite`. Analysis lives separately in `~/Library/Caches/Katastro/analysis.sqlite`; deleting this cache does not delete variations. `KATASTRO_DATA_DIR` and `KATASTRO_CACHE_DIR` override these directories for isolated runs. Cache compatibility includes game history, rules, komi, setup, side to move, model and executable checksums, and engine settings.

## Development and validation

Every feature starts with a failing behavioral integration test. Tests use actual SGF parsing, temporary SQLite databases, a controllable subprocess, and real UI input. [AGENTS.md](AGENTS.md) requires counterfactual checks of test strength. [Validation evidence](docs/validation.md) records the red/green cycles, mutation checks, real-engine runs, and measured latency.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The portable suite has 21 integration tests. Three additional checks require local assets and run explicitly:

```sh
export KATASTRO_TEST_ENGINE=/opt/homebrew/bin/katago
export KATASTRO_TEST_MODEL="$HOME/katrain/katrain/models/b10c384h6nbttflrs.bin.gz"
export KATASTRO_SGF_DIR="$HOME/Downloads"
cargo test --release --test engine live_katago -- --ignored --nocapture
cargo test --release --test corpus -- --ignored --nocapture
cargo test --release --test performance -- --ignored --nocapture
```

The MVP supports single-game, square SGFs from 2×2 through 19×19, captures, ko/superko, passes, handicap setup, imported branches, comments, and common text encodings. Missing rules default visibly to Chinese. Midgame setup edits remain viewable but their subsequent analysis is unavailable rather than inventing history. Automatic refinement currently stops at 64 visits; configurable deeper budgets, ownership maps, candidate moves, editing game properties, and SGF collections are future work.

Read the [framework comparison and source findings](docs/research.md), [architecture and acceptance plan](docs/design.md), and [original engine-only measurements](docs/engine-probe.json).
