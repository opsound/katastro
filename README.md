# Katastro

Native macOS Go review app in Rust, powered by KataGo.

- Cached points and winrate charts that refine in the background.
- Autosaved variations in a tree that preserves the original game.
- AI move suggestions and optional group strength overlays.

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

## Storage

- Reviews: `~/Library/Application Support/Katastro/reviews.sqlite`
- Analysis cache: `~/Library/Caches/Katastro/analysis.sqlite`

## Docs

[Architecture](docs/design.md) · [Framework research](docs/research.md) · [Testing and validation](docs/validation.md) · [Contribution rules](AGENTS.md)
