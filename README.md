# Katastro

Native macOS Go review app in Rust, powered by KataGo.

- Cached points and winrate charts that refine in the background.
- Autosaved variations in a tree that preserves the original game.
- AI move suggestions and optional group strength overlays.
- Automatic Fox and Pandanet downloads and a searchable local game library.
- Play as Black or White against a human-like opponent from 20 kyu to 9 dan.
- Training from costly mistakes in your games, with a 1-dan opponent and blind retries.

## Run

On an Apple Silicon Mac running macOS 13 or later, install Rust 1.95 or newer and the Xcode command-line tools. The first desktop build downloads a SHA-256-pinned runtime (~192 MiB) containing KataGo 1.18.2, a review model, and the HumanSL model. KataGo runs locally using Metal.

```sh
cargo run --release
# Or open a game directly
cargo run --release -- game.sgf
```

To create a movable, ad-hoc-signed app with everything needed for offline review, training, and play, run

```sh
bash scripts/package-macos.sh
open local/Katastro.app
```

An offline rebuild can use the populated Cargo and runtime caches with `KATASTRO_OFFLINE=1 cargo build --locked --offline`. The runtime archive lives in `~/Library/Caches/Katastro/build-runtime`. Set `KATASTRO_RUNTIME_CACHE` to use a different directory. The engine and models are GitHub Release assets. Their pinned source recipe and checksums live in `runtime/`. The repository contains no submodules or engine/model binaries.

**Engine** lets you override the bundled executable or review model and adjust search effort and parallelism. [Engine settings](docs/engine-settings.md)

## Play against AI

Choose **Play AI**, select an opponent rank, your color and a 9×9, 13×13 or 19×19 board, then choose **Start game**. The default is 5 kyu on 19×19 with you playing Black. Each rank selects KataGo’s modern HumanSL profile and samples its predicted human moves. The ranks approximate playing style and difficulty and can differ from server ratings. [KataGo’s HumanSL analysis guide](https://github.com/lightvector/KataGo/blob/v1.18.2/docs/Analysis_Engine.md#human-sl-analysis-guide) · [Upstream 5-kyu configuration](https://github.com/lightvector/KataGo/blob/v1.18.2/cpp/configs/gtp_human5k_example.cfg)

Games use Chinese area scoring with 7.5 komi. Capture disputed or dead stones before passing. Two consecutive passes count the position on the board. Resignation also records a result. Every move is saved in a local SGF. Choose **Review game** at the end to analyze it and explore variations.

## Controls

Click the board to explore a variation. The tree keeps the played game on top and shows each move’s board coordinate below its numbered stone. Click a stone or its coordinate to revisit the position. A gold line traces the selected variation back to its fork. You can also navigate by clicking a chart.

| Input | Action |
| --- | --- |
| Left / Right | Previous / next move |
| Up / Down | Switch variation rows |
| Home / End | Root / last original move |
| P | Pass |
| Space | Analyze / pause |
| Cmd+O / Cmd+S | Open / export SGF |

## Game libraries

For Fox, open **Games → Sync settings → Fox sync** and enter your Fox username. Use the server filter in **Games** to browse either archive or both. See [Fox sync](docs/fox-sync.md).

For Pandanet, click **Pandanet** at the bottom right, enter your login, and choose **Connect and sync**. **Remember login in Keychain** enables automatic sign-in after reopening. With automatic sync enabled, Katastro checks on startup and every 15 minutes while the app is open.

Click **Games** in the toolbar to browse downloaded games in a table with date, players, ranks, winner, win condition, and server. Click any heading to sort; click again to reverse. Player columns sort by name, and point margins sort numerically. A small gold trophy marks your wins. The table grows vertically with the library window, including when sync settings are open. Search by player/date/result, resize columns as needed, and click a row to open its review. **Sync history** defaults to the latest 200 games; choose another limit or all games. Expanding it downloads older records; lowering it keeps existing downloads.

Downloads run two at a time. Existing game IDs with local SGFs are skipped, and failed downloads are retried. Routine checks stop at a fully downloaded archive page; a daily scan checks the selected history range for gaps. Downloaded originals are preserved separately from autosaved review variations.

## Training

Click **Train** or **Games → Train from these games**. Exercises are prepared from completed review analysis and from the synced library while no game is open, then cached for quick starts. Setup uses your latest 20 games by default, with randomized game selection and repeated misses of the same key move grouped into one problem. Play five or ten moves against a 1-dan HumanSL opponent, on positions where one of your moves lost at least 5 points, then compare the attempt with the recorded continuation in an overlaid score graph. Training uses points only. Passing depends on reducing cumulative point losses on your own moves; opponent blunders and later gains do not cancel earlier mistakes.

Retry without hints after a failure, choose **Next problem** after any result or while exploring, or pass/give up to explore suggested lines in the normal reviewer. Training uses the bundled HumanSL model. See the [training policy and test instructions](docs/training.md).

## Storage

- Reviews and completed training attempts: `~/Library/Application Support/Katastro/reviews.sqlite`
- Training review SGFs: `~/Library/Application Support/Katastro/Training/`
- Games against the AI: `~/Library/Application Support/Katastro/Played Games/`
- Fox and Pandanet originals and sync indexes are in `~/Library/Application Support/Katastro/Fox/` and `~/Library/Application Support/Katastro/Pandanet/`
- Remembered Pandanet password: macOS login Keychain
- Analysis cache: `~/Library/Caches/Katastro/analysis.sqlite`

## Docs

[Architecture](docs/design.md) · [Framework research](docs/research.md) · [Testing and validation](docs/validation.md) · [Contribution rules](AGENTS.md)
