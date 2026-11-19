# Katastro

Native macOS Go review app in Rust, powered by KataGo.

- Cached points and winrate charts that refine in the background.
- Autosaved variations in a tree that preserves the original game.
- AI move suggestions and optional group strength overlays.
- Automatic Fox and Pandanet downloads and a searchable local game library.
- Training from costly mistakes in your games, with a 1-dan opponent and blind retries.

## Run

Requires Rust. For analysis, install KataGo and a model, then choose their paths in **Engine settings**. The pane also controls the review visit limit, parallel positions, and search threads per position, with saved preferences and a reset to defaults. See [engine settings](docs/engine-settings.md) for the tradeoffs.

```sh
cargo run --release -- game.sgf
```

Or build a macOS app:

```sh
bash scripts/package-macos.sh
open local/Katastro.app
```

## Controls

Click the board to explore a variation; click the tree or a chart to revisit a position.

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

Retry without hints after a failure, choose **Next problem** after any result or while exploring, or pass/give up to explore suggested lines in the normal reviewer. The opponent model downloads once (99 MB); games stay local. See the [training policy and test instructions](docs/training.md).

## Storage

- Reviews and completed training attempts: `~/Library/Application Support/Katastro/reviews.sqlite`
- Training review SGFs and opponent model: `~/Library/Application Support/Katastro/Training/` and `models/`
- Fox and Pandanet originals and sync indexes are in `~/Library/Application Support/Katastro/Fox/` and `~/Library/Application Support/Katastro/Pandanet/`
- Remembered Pandanet password: macOS login Keychain
- Analysis cache: `~/Library/Caches/Katastro/analysis.sqlite`

## Docs

[Architecture](docs/design.md) · [Framework research](docs/research.md) · [Testing and validation](docs/validation.md) · [Contribution rules](AGENTS.md)
