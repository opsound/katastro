//! One played game owns its own engine, command stream, and autosaved SGF.
use crate::{
    Board, Color, Document, NodeId, Point, Result,
    engine::{Engine, EngineConfig, EngineEvent},
    training::{sample_policy, two_passes},
    updates::{self, Wake},
};
use serde_json::json;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant},
};

pub(crate) const RANKS: [&str; 29] = [
    "20k", "19k", "18k", "17k", "16k", "15k", "14k", "13k", "12k", "11k", "10k", "9k", "8k", "7k",
    "6k", "5k", "4k", "3k", "2k", "1k", "1d", "2d", "3d", "4d", "5d", "6d", "7d", "8d", "9d",
];
#[derive(Clone, Copy)]
pub(crate) struct Settings {
    pub rank: &'static str,
    pub player: Color,
    pub size: usize,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            rank: "5k",
            player: Color::Black,
            size: 19,
        }
    }
}
#[derive(Clone)]
pub(crate) struct State {
    pub doc: Document,
    pub board: Board,
    pub path: PathBuf,
    pub status: String,
    pub your_turn: bool,
    pub finished: bool,
}
pub(crate) enum Input {
    Play(NodeId, Option<Point>),
    Resign,
}
pub(crate) struct Client {
    pub input: Sender<Input>,
    pub states: Receiver<std::result::Result<State, String>>,
}
impl Client {
    pub fn start(
        settings: Settings,
        config: EngineConfig,
        folder: PathBuf,
        human: PathBuf,
        wake: Wake,
    ) -> Self {
        let (input, rx) = mpsc::channel();
        let (publish, states) = updates::channel(wake);
        thread::spawn(move || {
            if let Err(e) = run(settings, config, folder, &human, rx, &publish) {
                publish(Err(e.to_string()));
            }
        });
        Self { input, states }
    }
}
fn save(path: &Path, doc: &Document) -> Result<()> {
    let mut tmp = tempfile::NamedTempFile::new_in(path.parent().ok_or("Missing game folder")?)?;
    tmp.write_all(doc.to_sgf().as_bytes())?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)?;
    Ok(())
}
fn run(
    s: Settings,
    config: EngineConfig,
    folder: PathBuf,
    human: &Path,
    rx: Receiver<Input>,
    publish: &impl Fn(std::result::Result<State, String>),
) -> Result<()> {
    let (players, rank) = if s.player == Color::Black {
        ("PB[You]PW[KataGo]", "WR")
    } else {
        ("PB[KataGo]PW[You]", "BR")
    };
    let mut doc = Document::parse(
        format!(
            "(;GM[1]FF[4]SZ[{}]RU[Chinese]KM[7.5]{players}{rank}[{}])",
            s.size, s.rank
        )
        .as_bytes(),
    )?;
    let mut engine = Engine::spawn_with_human(&config, human)?;
    fs::create_dir_all(&folder)?;
    let mut file = tempfile::Builder::new()
        .prefix("Katastro-")
        .suffix(".sgf")
        .tempfile_in(&folder)?;
    file.write_all(doc.to_sgf().as_bytes())?;
    let (_, path) = file.keep()?;
    let mut resigned = false;
    loop {
        let board = doc.board(doc.selected)?;
        let finished = resigned || two_passes(&doc);
        let your_turn = !finished && board.next == s.player;
        let mut state = State {
            doc: doc.clone(),
            board,
            path: path.clone(),
            your_turn,
            finished,
            status: if resigned {
                "You resigned".into()
            } else if finished {
                "Both players passed".into()
            } else if your_turn {
                "Your turn".into()
            } else {
                format!("KataGo ({}) is thinking", s.rank)
            },
        };
        publish(Ok(state.clone()));
        if finished {
            return Ok(());
        }
        resigned = if your_turn {
            loop {
                match rx.recv() {
                    Ok(Input::Resign) => break true,
                    Ok(Input::Play(id, point)) if id == doc.selected => {
                        match doc.append_move(point) {
                            Ok(_) => break false,
                            Err(e) => {
                                state.status = e.to_string();
                                publish(Ok(state.clone()));
                            }
                        }
                    }
                    Ok(Input::Play(..)) => {}
                    Err(_) => return Ok(()),
                }
            }
        } else {
            let id = format!("opponent-{}", doc.selected);
            let position = doc.position(doc.selected)?;
            let turn = position.moves.len();
            let mut q = serde_json::to_value(position)?;
            q["id"] = json!(&id);
            q["maxVisits"] = json!(1);
            q["includePolicy"] = json!(true);
            q["analyzeTurns"] = json!([turn]);
            q["overrideSettings"] = json!({
                "humanSLProfile": format!("rank_{}", s.rank),
                "ignorePreRootHistory": false,
            });
            engine.send(&q)?;
            let deadline = Instant::now() + Duration::from_secs(120);
            loop {
                match rx.recv_timeout(Duration::from_millis(12)) {
                    Ok(Input::Resign) => break true,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
                    _ => {}
                }
                if Instant::now() >= deadline {
                    return Err("KataGo timed out waiting for a move".into());
                }
                let Some(event) = engine.try_recv() else {
                    continue;
                };
                match event {
                    EngineEvent::Response(r) if r["id"] == id => {
                        if let Some(e) = r.get("error") {
                            return Err(format!("KataGo: {e}").into());
                        }
                        if r["turnNumber"] != turn || r["isDuringSearch"] != false {
                            continue;
                        }
                        let point = sample_policy(&doc, &r["humanPolicy"], rand::random())?;
                        doc.append_move(point)?;
                        break false;
                    }
                    EngineEvent::Fault(e) => return Err(e.into()),
                    EngineEvent::Exited => return Err("KataGo exited during the game".into()),
                    _ => {}
                }
            }
        };
        if resigned {
            doc.nodes[0]
                .props
                .insert("RE".into(), vec![format!("{}+R", s.player.other().code())]);
        } else {
            doc.mainline.push(doc.selected);
            if two_passes(&doc) {
                let lead = doc.board(doc.selected)?.area_lead(doc.komi);
                let winner = if lead > 0.0 {
                    Color::Black
                } else {
                    Color::White
                };
                let result = format!("{}+{}", winner.code(), lead.abs());
                doc.nodes[0].props.insert("RE".into(), vec![result]);
            }
        }
        save(&path, &doc)?;
    }
}
