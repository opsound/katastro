# Katastro

Native macOS Go review app in Rust, powered by KataGo.

- Cached points and winrate charts that refine in the background.
- Autosaved variations in a tree that preserves the original game.
- AI move suggestions and optional group strength overlays.
- Automatic Pandanet downloads and a searchable local game library.

## Run

Requires Rust. For analysis, install KataGo and a model, then choose their paths in **Engine settings**.

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

## Pandanet games

Click **Pandanet** at the bottom right, enter your login, and choose **Connect and sync**. **Remember login in Keychain** enables automatic sign-in after reopening. With automatic sync enabled, Katastro checks on startup and every 15 minutes while the app is open.

Click **Games** in the toolbar to browse downloaded games in a table: date, White, White rank, Black, Black rank, winner, and win condition. Click any heading to sort; click again to reverse. Ranks sort by strength and point margins numerically. Search by player/date/result, resize columns as needed, and click a row to open its review. **Sync history** defaults to the latest 200 games; choose another limit or all games. Expanding it downloads older records; lowering it keeps existing downloads.

Downloads run two at a time. Existing game IDs with local SGFs are skipped, and failed downloads are retried. Routine checks stop at a fully downloaded archive page; a daily scan checks the selected history range for gaps. Downloaded originals are preserved separately from autosaved review variations.

## Storage

- Reviews: `~/Library/Application Support/Katastro/reviews.sqlite`
- Pandanet originals and sync index: `~/Library/Application Support/Katastro/Pandanet/`
- Remembered Pandanet password: macOS login Keychain
- Analysis cache: `~/Library/Caches/Katastro/analysis.sqlite`

## Docs

[Architecture](docs/design.md) · [Framework research](docs/research.md) · [Testing and validation](docs/validation.md) · [Contribution rules](AGENTS.md)
