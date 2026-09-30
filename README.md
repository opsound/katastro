# Katastro

A native macOS Go game review app written in Rust. GPU-rendered controls are acceptable. The initial technology recommendation is **egui/eframe with wgpu's Metal backend**, a persistent KataGo analysis subprocess, and SQLite persistence.

This repository currently contains research and implementation requirements. App implementation has not started.

The minimum product must:

- Restore compatible cached analysis when an SGF is reopened.
- Fill a points and winrate chart across the game quickly with shallow analysis, then replace estimates as deeper analysis arrives.
- Allow the user to play a variation from any position, including while analysis is running, and persist every resulting branch.
- Display the game tree from left to right, with the original SGF main line fixed on the top row.
- Develop every feature test first, with integration tests that demonstrate observable user benefit and fail when that behavior is removed.

Read the [framework comparison and source findings](docs/research.md), [architecture and acceptance tests](docs/design.md), and [measured engine results](docs/engine-probe.json). [AGENTS.md](AGENTS.md) makes the development requirements persistent for future work.

The first implementation slice is SGF import, legal variation creation, and save/reopen through the application API. Its integration tests precede production code. The GUI and analysis scheduler then use that same tested application layer.
