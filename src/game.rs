use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
mod board;
mod sgf;
pub use board::Board;
use board::BoardHistory;
pub use sgf::normalize_rules;
pub type NodeId = usize;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Color {
    Black,
    White,
}
impl Color {
    pub fn other(self) -> Self {
        match self {
            Self::Black => Self::White,
            Self::White => Self::Black,
        }
    }
    pub fn code(self) -> &'static str {
        match self {
            Self::Black => "B",
            Self::White => "W",
        }
    }
    /// Convert a Black-relative score or ownership value to this color.
    pub(crate) fn perspective(self, value: f64) -> f64 {
        if self == Self::Black { value } else { -value }
    }
    pub fn index(self) -> usize {
        usize::from(self == Self::White)
    }
}
impl std::fmt::Display for Color {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Black => "Black",
            Self::White => "White",
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Point {
    pub x: usize,
    pub y: usize,
}
impl Point {
    pub fn new(x: usize, y: usize) -> Self {
        Self { x, y }
    }
    pub fn sgf(self) -> String {
        format!(
            "{}{}",
            (b'a' + self.x as u8) as char,
            (b'a' + self.y as u8) as char
        )
    }
    pub fn gtp(self, size: usize) -> String {
        format!(
            "{}{}",
            b"ABCDEFGHJKLMNOPQRST"[self.x] as char,
            size - self.y
        )
    }
    pub fn from_gtp(text: &str, size: usize) -> Result<Option<Self>> {
        if text.eq_ignore_ascii_case("pass") {
            return Ok(None);
        }
        let bytes = text.as_bytes();
        let x = bytes.first().and_then(|letter| {
            b"ABCDEFGHJKLMNOPQRST"
                .iter()
                .position(|c| *c == letter.to_ascii_uppercase())
        });
        let y = text.get(1..).and_then(|s| s.parse::<usize>().ok());
        match (x, y) {
            (Some(x), Some(row)) if x < size && (1..=size).contains(&row) => {
                Ok(Some(Self::new(x, size - row)))
            }
            _ => Err(format!("Invalid GTP coordinate: {text}").into()),
        }
    }
    fn parse(text: &str, size: usize) -> Result<Option<Self>> {
        if text.is_empty() || (text == "tt" && size <= 19) {
            return Ok(None);
        }
        let b = text.as_bytes();
        if b.len() != 2 || !b.iter().all(|c| c.is_ascii_lowercase()) {
            return Err(format!("Invalid SGF coordinate: {text}").into());
        }
        let point = Self::new((b[0] - b'a') as usize, (b[1] - b'a') as usize);
        if point.x >= size || point.y >= size {
            return Err(format!("Move {text} is outside the board").into());
        }
        Ok(Some(point))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Move {
    pub color: Color,
    pub point: Option<Point>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Node {
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub props: BTreeMap<String, Vec<String>>,
    pub played: Option<Move>,
}
impl Node {
    pub fn property(&self, key: &str) -> Option<&str> {
        self.props.get(key)?.first().map(String::as_str)
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Document {
    pub nodes: Vec<Node>,
    pub mainline: Vec<NodeId>,
    pub selected: NodeId,
    pub size: usize,
    pub rules: String,
    pub komi: f64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodePosition {
    pub id: NodeId,
    pub column: usize,
    pub row: usize,
}
impl Document {
    pub fn path(&self, node: NodeId) -> Result<Vec<NodeId>> {
        if node >= self.nodes.len() {
            return Err("Unknown tree node".into());
        }
        let mut path = vec![node];
        let mut current = node;
        while let Some(parent) = self.nodes[current].parent {
            path.push(parent);
            current = parent;
        }
        path.reverse();
        Ok(path)
    }
    fn empty_board(&self) -> Board {
        let root = &self.nodes[0];
        let initial = if root.property("PL") == Some("W")
            || (root.property("PL").is_none()
                && root
                    .property("HA")
                    .and_then(|h| h.parse::<usize>().ok())
                    .is_some_and(|h| h >= 2))
        {
            Color::White
        } else {
            Color::Black
        };
        Board {
            size: self.size,
            cells: vec![None; self.size * self.size],
            next: initial,
            move_number: 0,
            captures: [0, 0],
        }
    }
    pub(crate) fn initial_board(&self) -> Result<Board> {
        let mut board = self.empty_board();
        Self::apply_setup(&self.nodes[0], &mut board)?;
        Ok(board)
    }
    fn apply_setup(n: &Node, board: &mut Board) -> Result<bool> {
        let mut setup = false;
        for (property, color) in [
            ("AE", None),
            ("AB", Some(Color::Black)),
            ("AW", Some(Color::White)),
        ] {
            if let Some(points) = n.props.get(property) {
                for point in points {
                    let point =
                        Point::parse(point, board.size)?.ok_or("Setup stone cannot be pass")?;
                    board.cells[point.y * board.size + point.x] = color;
                    setup = true;
                }
            }
        }
        if let Some(next) = n.property("PL") {
            board.next = if next == "B" {
                Color::Black
            } else {
                Color::White
            };
            setup = true;
        }
        Ok(setup)
    }
    fn apply_node(&self, id: NodeId, board: &mut Board, history: &mut BoardHistory) -> Result<()> {
        let node = &self.nodes[id];
        if Self::apply_setup(node, board)? {
            history.clear();
            history.push((board.cells.clone(), board.next));
        }
        if let Some(played) = node.played {
            board.apply(played, &self.rules, history)?;
            history.push((board.cells.clone(), board.next));
        }
        Ok(())
    }
    fn replay(
        &self,
        node: NodeId,
        mut visit: impl FnMut(NodeId, &Board),
    ) -> Result<(Board, BoardHistory)> {
        let mut board = self.empty_board();
        let mut history = vec![(board.cells.clone(), board.next)];
        for id in self.path(node)? {
            self.apply_node(id, &mut board, &mut history)?;
            visit(id, &board);
        }
        Ok((board, history))
    }
    fn validate_tree(&self) -> Result<()> {
        // Replay each edge once; only forks need independent board/history copies.
        fn line(
            doc: &Document,
            mut id: NodeId,
            mut board: Board,
            mut history: BoardHistory,
        ) -> Result<()> {
            loop {
                doc.apply_node(id, &mut board, &mut history)?;
                match doc.nodes[id].children.as_slice() {
                    [] => return Ok(()),
                    [child] => id = *child,
                    children => {
                        for child in children {
                            line(doc, *child, board.clone(), history.clone())?;
                        }
                        return Ok(());
                    }
                }
            }
        }
        let board = self.empty_board();
        let history = vec![(board.cells.clone(), board.next)];
        line(self, 0, board, history)
    }
    pub fn board(&self, node: NodeId) -> Result<Board> {
        Ok(self.replay(node, |_, _| {})?.0)
    }
    /// Number surviving moves after an anchor on this path. Passes count, but
    /// setup stones and positions outside the anchor's descendants stay blank.
    pub fn numbered_board(
        &self,
        node: NodeId,
        anchor: NodeId,
    ) -> Result<(Board, Vec<Option<usize>>)> {
        let mut labels = vec![None; self.size * self.size];
        let mut number = None;
        let (board, _) = self.replay(node, |id, board| {
            if id == anchor {
                number = Some(0);
            } else if let Some(number) = &mut number {
                for (label, stone) in labels.iter_mut().zip(&board.cells) {
                    if label.is_some_and(|(color, _)| Some(color) != *stone) {
                        *label = None;
                    }
                }
                let node = &self.nodes[id];
                for property in ["AE", "AB", "AW"] {
                    for point in node.props.get(property).into_iter().flatten() {
                        if let Ok(Some(point)) = Point::parse(point, self.size) {
                            labels[point.y * self.size + point.x] = None;
                        }
                    }
                }
                if let Some(played) = node.played {
                    *number += 1;
                    if let Some(point) = played.point
                        && board.stone(point) == Some(played.color)
                    {
                        labels[point.y * self.size + point.x] = Some((played.color, *number));
                    }
                }
            }
        })?;
        Ok((
            board,
            labels.into_iter().map(|v| v.map(|(_, n)| n)).collect(),
        ))
    }
    /// Follow the imported game when present, otherwise this branch's first child.
    pub fn continuation(&self, node: NodeId) -> Option<NodeId> {
        let children = &self.nodes.get(node)?.children;
        self.mainline
            .windows(2)
            .find(|pair| pair[0] == node)
            .map(|pair| pair[1])
            .filter(|id| children.contains(id))
            .or_else(|| children.first().copied())
    }
    fn ordered_children(&self, node: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let continuation = self.continuation(node);
        continuation.into_iter().chain(
            self.nodes[node]
                .children
                .iter()
                .copied()
                .filter(move |child| Some(*child) != continuation),
        )
    }
    pub fn append_move(&mut self, point: Option<Point>) -> Result<NodeId> {
        let (mut board, history) = self.replay(self.selected, |_, _| {})?;
        let played = Move {
            color: board.next,
            point,
        };
        board.apply(played, &self.rules, &history)?;
        if let Some(existing) = self.nodes[self.selected]
            .children
            .iter()
            .find(|id| self.nodes[**id].played == Some(played))
        {
            self.selected = *existing;
            return Ok(*existing);
        }
        let parent = self.selected;
        let id = self.nodes.len();
        let props = BTreeMap::from([(
            played.color.code().into(),
            vec![point.map(Point::sgf).unwrap_or_default()],
        )]);
        self.nodes.push(Node {
            parent: Some(parent),
            children: Vec::new(),
            props,
            played: Some(played),
        });
        self.nodes[parent].children.push(id);
        self.selected = id;
        Ok(id)
    }
    pub fn vertical_neighbor(&self, down: bool) -> Option<NodeId> {
        let layout = self.layout();
        let current = &layout[self.selected];
        layout
            .iter()
            .filter(|p| {
                p.column == current.column
                    && if down {
                        p.row > current.row
                    } else {
                        p.row < current.row
                    }
            })
            .min_by_key(|p| p.row.abs_diff(current.row))
            .map(|p| p.id)
    }
    pub fn layout(&self) -> Vec<NodePosition> {
        let mut positions = vec![
            NodePosition {
                id: 0,
                column: 0,
                row: 0
            };
            self.nodes.len()
        ];
        let mut next_row = 1;
        fn visit(
            doc: &Document,
            id: NodeId,
            col: usize,
            row: usize,
            positions: &mut [NodePosition],
            next_row: &mut usize,
        ) {
            positions[id] = NodePosition {
                id,
                column: col,
                row,
            };
            for (index, child) in doc.ordered_children(id).enumerate() {
                let child_row = if index == 0 {
                    row
                } else {
                    let row = *next_row;
                    *next_row += 1;
                    row
                };
                visit(doc, child, col + 1, child_row, positions, next_row);
            }
        }
        visit(self, 0, 0, 0, &mut positions, &mut next_row);
        positions
    }
}
