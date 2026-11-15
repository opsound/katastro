use katastro::{
    Point,
    engine::EngineConfig,
    worker::{Client, Command, Snapshot},
};
use std::{path::Path, time::Duration};

fn client(root: &Path) -> Client {
    let client = Client::spawn(
        root.join("reviews"),
        root.join("cache"),
        EngineConfig {
            analysis: Default::default(),
            executable: Default::default(),
            model: Default::default(),
        },
    );
    client.recv(Duration::from_secs(5)).unwrap();
    client
}
fn command(client: &Client, command: Command) -> Snapshot {
    client.send(command).unwrap();
    let state = client.recv(Duration::from_secs(5)).unwrap();
    assert!(state.error.is_none(), "{:?}", state.error);
    state
}
fn labels(state: &Snapshot) -> Vec<(usize, usize)> {
    state
        .move_numbers
        .iter()
        .enumerate()
        .filter_map(|(i, n)| n.map(|n| (i, n)))
        .collect()
}
// Benefit: move labels tell the actual branch sequence even through captures,
// passes and navigation. A stale map can label captured or unrelated stones.
#[test]
fn relative_numbers_follow_captures_passes_branches_and_reopening() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("game.sgf");
    let sgf = b"(;SZ[5]AW[ab][ba][cb]AB[dd];B[ee];C[comment];W[de];B[ca])";
    std::fs::write(&source, sgf).unwrap();
    let worker = client(temp.path());
    command(&worker, Command::Open(source.clone()));
    command(&worker, Command::NumberFrom(Some(3)));
    command(&worker, Command::Select(3));
    let first = command(&worker, Command::Play(Some(Point::new(1, 1))));
    assert_eq!(
        labels(&first),
        vec![(6, 1)],
        "first move after anchor must be 1"
    );
    let first_node = first.document.unwrap().selected;
    let capture = command(&worker, Command::Play(Some(Point::new(1, 2))));
    assert_eq!(
        labels(&capture),
        vec![(11, 2)],
        "captured move 1 must disappear"
    );
    command(&worker, Command::Play(None));
    let replacement = command(&worker, Command::Play(Some(Point::new(1, 1))));
    assert_eq!(
        labels(&replacement),
        vec![(6, 4), (11, 2)],
        "pass counts; reoccupation has its own number"
    );
    let last = replacement.document.unwrap().selected;
    let previous = command(&worker, Command::Select(first_node));
    assert_eq!(
        labels(&previous),
        vec![(6, 1)],
        "navigation must restore captured stones and their labels"
    );
    command(&worker, Command::Select(3));
    let alternate = command(&worker, Command::Play(Some(Point::new(2, 2))));
    assert_eq!(
        labels(&alternate),
        vec![(12, 1)],
        "another line starts at the same anchor"
    );
    assert!(labels(&command(&worker, Command::Select(1))).is_empty());
    let restored = command(&worker, Command::Select(last));
    assert_eq!(labels(&restored), vec![(6, 4), (11, 2)]);
    assert_eq!(restored.document.unwrap().mainline, vec![0, 1, 2, 3, 4]);
    command(&worker, Command::NumberFrom(Some(first_node)));
    let unrelated = command(
        &worker,
        Command::Select(alternate.document.unwrap().selected),
    );
    assert!(
        labels(&unrelated).is_empty(),
        "sibling outside anchor path must not be numbered"
    );
    command(&worker, Command::NumberFrom(Some(3)));
    let opened = command(&worker, Command::Open(source.clone()));
    let count = opened.document.unwrap().nodes.len();
    assert!(
        labels(&command(&worker, Command::Select(last))).is_empty(),
        "opening a document clears transient numbering"
    );
    drop(worker);
    let reopened = client(temp.path());
    let restored = command(&reopened, Command::Open(source.clone()));
    assert_eq!(
        restored.document.unwrap().nodes.len(),
        count,
        "explored variations still autosave"
    );
    assert!(labels(&command(&reopened, Command::Select(last))).is_empty());
    assert_eq!(std::fs::read(&source).unwrap(), sgf, "source is unchanged");
}

#[test]
fn comments_do_not_consume_numbers_and_setup_replacements_are_unnumbered() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("annotated.sgf");
    std::fs::write(
        &source,
        b"(;SZ[5];B[aa];W[ee];B[bb];C[annotation];W[cc];AE[bb]AB[bb];B[dd])",
    )
    .unwrap();
    let worker = client(temp.path());
    command(&worker, Command::Open(source));
    command(&worker, Command::NumberFrom(Some(2)));
    assert_eq!(labels(&command(&worker, Command::Select(4))), vec![(6, 1)]);
    assert_eq!(
        labels(&command(&worker, Command::Select(5))),
        vec![(6, 1), (12, 2)]
    );
    assert_eq!(labels(&command(&worker, Command::Select(6))), vec![(12, 2)]);
    assert_eq!(
        labels(&command(&worker, Command::Select(7))),
        vec![(12, 2), (18, 3)]
    );
}
