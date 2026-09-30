# Rust framework research for Katastro

Research date: September 29, 2026. The recommendation below is based on current primary documentation, inspection of the user's KataGo and KaTrain checkouts, and a local engine experiment. GUI frameworks have not been benchmarked or compiled against this app yet.

## Recommendation

Start with **egui/eframe**, explicitly using **wgpu and Metal** for rendering. The app's distinctive screens are a custom Go board, a changing analysis chart, and an interactive variation tree. egui offers direct 2D painting, an existing plotting library, and a UI testing harness. This is the best initial fit in my assessment, particularly because integration testing is a project requirement. Its controls will need macOS styling and explicit keyboard, menu, file-dialog, and accessibility work.

Keep **GPUI** as the strongest alternative if macOS integration and the feel of the desktop shell become decisive. Keep the application layer independent so a framework change does not require rewriting cache, tree, or scheduler logic.

## Framework comparison

The fit assessments are project-specific judgments, not measured performance rankings. All four can build compiled desktop interfaces with custom rendering.

| Framework | Relevant capabilities | Testing support | Project assessment |
| --- | --- | --- | --- |
| egui and eframe | Immediate-mode Rust UI, direct 2D painting, wgpu rendering, AccessKit support; `egui_plot` supplies interactive plots | `egui_kittest` supports input through accessibility semantics, application harnesses, and optional image comparisons | Recommended for this board, chart, and tree workflow. Fast iteration and useful tests; native appearance is explicitly outside egui's goals |
| GPUI | Hybrid retained and immediate UI; macOS Metal renderer, platform services, custom canvas/elements, actions and asynchronous execution | `#[gpui::test]`, `TestAppContext`, simulated platform input | Strong macOS-oriented alternative. More framework coupling, changing APIs, and setup work; custom chart code would need evaluation |
| Iced | Elm-style messages and state updates, custom Canvas, wgpu rendering | `iced_test` offers a headless simulator with input interaction | Strong choice for explicit asynchronous state transitions. Assess custom canvas testing and macOS menu integration in a spike |
| Slint | Declarative `.slint` UI with Rust logic; Winit backend supports Metal through Skia or FemtoVG/wgpu; stable public 1.x API | Testing backend offers headless input and simulated time | Credible for designed layouts. Adds another UI language; custom board/tree ergonomics need evaluation. Internal test APIs require matching exact versions |

egui's [official overview](https://github.com/emilk/egui) documents custom painting, accessibility, and its appearance limitations. [eframe](https://docs.rs/eframe/latest/eframe/) can use wgpu; [wgpu](https://docs.rs/wgpu/latest/wgpu/) supports Metal. Make backend selection explicit and verify the chosen adapter on macOS. The [plotting library](https://github.com/emilk/egui_plot) and [egui_kittest](https://docs.rs/egui_kittest/latest/egui_kittest/) cover charting and UI testing. A painted canvas does not automatically supply useful semantic targets: board intersections and tree nodes need labels and actions that exercise real hit testing.

The [published GPUI 0.2.2 documentation](https://docs.rs/gpui/0.2.2/gpui/) describes Metal rendering and simulated-input tests, and requires Xcode for its macOS toolchain. The [upstream README](https://github.com/zed-industries/zed/blob/main/crates/gpui/README.md) now describes a separate `gpui_platform` setup. Pin a consistent published version or Git revision and use its corresponding documentation. The upstream project expressly warns that its APIs are pre-1.0 and may break. This machine currently selects the standalone Command Line Tools directory; full Xcode readiness has not been verified.

For Iced, see the [official project](https://github.com/iced-rs/iced), [Canvas API](https://docs.rs/iced/0.14.0/iced/widget/canvas/), and [headless test simulator](https://docs.rs/iced_test/0.14.0/iced_test/). For Slint, see the [official project](https://github.com/slint-ui/slint), [renderer matrix](https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backend_winit/), and [testing backend](https://docs.rs/i-slint-backend-testing/latest/i_slint_backend_testing/index.html). Slint's public API stability does not extend to that internal testing crate. Its [license options](https://github.com/slint-ui/slint/blob/master/LICENSE.md) also differ from the permissive options of many Rust libraries; select the applicable option before adoption.

A native AppKit wrapper using Objective-C bindings would be appropriate if system controls were required. The user confirmed that GPU-rendered Rust controls are acceptable, so there is no present reason to take on a full AppKit wrapper. Webview frameworks are outside the chosen rendering approach.

## KataGo findings

Inspected local checkout: `~/KataGo`, commit `d91ea855110dae533f0aada947b2b7d78cc8a4e1`. The installed Homebrew engine is a separate artifact: KataGo 1.18.2, compiled August 30, 2026, reporting the Metal backend. Its Git revision is omitted by the binary, so it must not be assumed identical to the checkout.

| Reference in the local checkout | Finding and implication |
| --- | --- |
| `cpp/neuralnet/metalbackend.swift`, `MPSGraphModelHandle` | Creates the default Metal device, command queue, and MPSGraph. This is the GPU inference entry point |
| `cpp/neuralnet/metalbackend.swift`, `createCoreMLComputeHandle` | Loads converted CoreML models with `.cpuAndNeuralEngine`, excluding the GPU from this path |
| `cpp/neuralnet/metalbackend.cpp`, `NeuralNet::createComputeHandle` | Dispatches between MPSGraph GPU and CoreML CPU/ANE handles; conversion uses the native `katagocoreml` library |
| `cpp/neuralnet/metallayers.swift`, `MPSGraphModel` | Constructs network graphs and uses `MPSNDArray`, buffer writes, graph execution, and result reads |
| `cpp/configs/analysis_example.cfg`, Metal section | Device index `0` selects GPU; `100` selects CPU/ANE. Describes a mixed four-server-thread configuration as a throughput option. In this checkout, the FP16 flag affects the ANE path; MPSGraph stays FP32 |
| `cpp/CMakeLists.txt`, Metal branch | Requires Ninja, Swift 5.9 or newer, AppleClang, and sets a macOS 13 deployment target for this build |
| `docs/Analysis_Engine.md`, `cpp/command/analysis.cpp` | Asynchronous JSON analysis over stdin/stdout, multiple turns, visit caps, priorities, intermediate reports, and termination |

These are useful inference hooks, not GUI drawing hooks. Use them through KataGo's analysis subprocess initially. The Rust GUI has its own Metal rendering path through wgpu. A subprocess also contains engine failures and allows independent engine upgrades. In-process C++/Swift integration would need a stable boundary and measurements showing a benefit before it becomes worthwhile.

The key protocol features are `analyzeTurns` for positions after 0 through N played moves, `maxVisits` for cheap passes, `reportDuringSearchEvery` for streaming, and `terminate` for active searches. Higher `priority` affects queued work; it does not interrupt an already running search. Refinement requests can benefit from the persistent engine's neural-network cache, but the protocol does not promise to resume a previous completed search tree or accumulate visits across independent requests. The durable app cache therefore stores returned results and their actual visits.

Configure `reportAnalysisWinratesAs = BLACK` and use `rootInfo.winrate` and `rootInfo.scoreLead` for the game chart. Missing values must remain gaps. Zero is valid data. Game history matters to ko rules and neural inputs; neither a filename nor KataGo's board hash is sufficient as the app's cache key. The [analysis protocol](https://github.com/lightvector/KataGo/blob/master/docs/Analysis_Engine.md) provides the public reference; the local checkout is the inspected implementation.

## KaTrain findings

Inspected local checkout: `~/katrain`, commit `f4981cf905cece90085ce4e3967415d0c16d525f`. KaTrain uses Kivy for its Python GUI and demonstrates a compatible separation between engine orchestration, game nodes, charts, and tree drawing.

- `katrain/core/engine.py` launches the analysis subprocess, reads stdout and stderr on separate threads, queues writes, correlates request IDs, and handles intermediate responses and cancellation. Its document-generation checks are useful precedents for ignoring late replies after switching games.
- `katrain/core/game_node.py` compresses analysis into SGF `KT` properties with a `KTV` version and can restore it later. This is useful persistence behavior, but is different from the automatic external cache requested here.
- `katrain/gui/widgets/movetree.py`, `draw_move_tree`, assigns horizontal depth and vertical lanes, visits ordered children, and draws branch edges. Adapt that layout concept while explicitly pinning our imported main line.
- `katrain/gui/widgets/graph.py` updates points and winrate data and coalesces redraws with Kivy's clock. Our chart should retain the original game's evaluations while showing a variation separately.

The engine shipped inside this local KaTrain checkout is an x86-64 Linux ELF binary. It is not the executable to use on this Mac. `/opt/homebrew/bin/katago` is the tested macOS engine. Use KaTrain's code as a behavioral reference; implement the Rust application independently.

## Local engine measurements

The user authorized SGFs in `~/Downloads`; 56 top-level lowercase `.sgf` files were found. Three simple 19x19 records were selected. All lacked `RU`, so Chinese rules were an explicit experimental assumption, with each SGF's komi retained. This is a timing probe, not a rules-correctness or analysis-accuracy validation.

Environment: Apple M4 Max, 64 GiB RAM, macOS 26.6.2; installed Metal KataGo 1.18.2; KaTrain's `b10c384h6nbttflrs.bin.gz` model. Eight analysis threads, one search thread per position, batch size eight, one GPU server thread. No GUI or durable analysis cache. One run per file and budget, using one persistent process and clearing its neural-network cache before each pass.

| SGF | Moves | All positions at 1 visit | At 8 visits | At 64 visits |
| --- | ---: | ---: | ---: | ---: |
| BPPVUFOBXN.sgf | 230 | 8.162 s | 9.299 s | 33.059 s |
| BTCWWDJUKP.sgf | 88 | 0.233 s | 1.664 s | 11.417 s |
| DSARLPYCMA.sgf | 220 | 0.597 s | 4.289 s | 32.693 s |

Every pass returned N+1 distinct positions with score, winrate, and the requested visit count. The first process acknowledged version after 0.335 seconds, but its first one-visit response took 4.055 seconds. Early passes include lazy inference warm-up effects that are not isolated by this experiment. The later games demonstrate promising warm-engine coverage, not a guaranteed cold-start latency. The probe also verified an intermediate search reply, termination acknowledgment, and final reply, together taking 0.062 seconds in that single trial.

The [raw measurement record](engine-probe.json) includes hashes, configuration, timing, and actual visits. Original SGFs and the model remain outside the repository. The probe did not measure cache reopening, visual chart latency, variation persistence, GPU/ANE comparisons, or quality against a stronger analysis baseline.

Based on these results, begin with a **one-visit coverage pass using the same model**, then increase search effort after coverage. Budgets such as 8, 64, and 512 visits are starting points to tune, not accuracy guarantees. Assess cold startup separately, retain the engine between games, and compare GPU-only, ANE-only, and mixed execution before claiming an optimal Metal configuration. A stronger second model would need a separate cache profile and explicit UI provenance.

## Group strength inspiration

The [r/baduk post by jc1030](https://www.reddit.com/r/baduk/comments/1wgbefn/i_made_a_tool_to_view_how_strong_and_weak_groups/) describes green, orange, and red outlines for expected survival, uncertain status, and expected capture. In the comments, the author confirms that coloring uses KataGo ownership and that strict connected chains avoid assuming connections between parts that might be sacrificed. The [linked interactive demo](https://jstnchng.github.io/go-review-timelapse/) documents owner-relative thresholds of +0.5/-0.5, with stronger outlines toward the endpoints. Its HTML and controls were inspected as a read-only reference; private demo data is not a repository fixture.

[KataGo's official analysis protocol](https://github.com/lightvector/KataGo/blob/master/docs/Analysis_Engine.md) provides root ownership with `includeOwnership`, as a board-sized row-major array in [-1, 1]. The perspective follows `reportAnalysisWinratesAs`; the local `cpp/search/searchresults.cpp` confirms sign inversion for Black output. Katastro already fixes that setting to BLACK, so White-chain readings must reverse the sign. The local KaTrain `core/engine.py` requests root ownership and `core/game_node.py` stores it with analysis, confirming that this fits a macOS review workflow. Root ownership avoids the larger payload of per-candidate ownership. The protocol documents extra memory use and some search overhead, so the first one-visit chart pass continues to omit it. No independent life-and-death solver is required for this estimated display.
