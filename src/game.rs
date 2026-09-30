use crate::Result;
use serde::{Deserialize, Serialize};
use sgf_parse::{SgfNode, SgfProp, go::Prop};
use std::collections::{BTreeMap, HashSet};
pub type NodeId = usize;
type BoardHistory = Vec<(Vec<Option<Color>>, Color)>;
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
    pub fn index(self) -> usize {
        usize::from(self == Self::White)
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
pub struct Board {
    pub size: usize,
    pub cells: Vec<Option<Color>>,
    pub next: Color,
    pub move_number: usize,
    pub captures: [usize; 2],
}
impl Board {
    pub fn stone(&self, point: Point) -> Option<Color> {
        if point.x >= self.size || point.y >= self.size {
            return None;
        }
        self.cells[point.y * self.size + point.x]
    }
    fn neighbors(&self, index: usize) -> Vec<usize> {
        let x = index % self.size;
        let y = index / self.size;
        let mut out = Vec::with_capacity(4);
        if x > 0 {
            out.push(index - 1);
        }
        if x + 1 < self.size {
            out.push(index + 1);
        }
        if y > 0 {
            out.push(index - self.size);
        }
        if y + 1 < self.size {
            out.push(index + self.size);
        }
        out
    }
    fn group(&self, start: usize) -> (Vec<usize>, bool) {
        let color = self.cells[start];
        let mut seen = HashSet::new();
        let mut stack = vec![start];
        let mut liberty = false;
        while let Some(index) = stack.pop() {
            if !seen.insert(index) {
                continue;
            }
            for neighbor in self.neighbors(index) {
                if self.cells[neighbor].is_none() {
                    liberty = true;
                } else if self.cells[neighbor] == color && !seen.contains(&neighbor) {
                    stack.push(neighbor);
                }
            }
        }
        (seen.into_iter().collect(), liberty)
    }
    fn apply(
        &mut self,
        played: Move,
        rules: &str,
        history: &[(Vec<Option<Color>>, Color)],
    ) -> Result<()> {
        if let Some(point) = played.point {
            if point.x >= self.size || point.y >= self.size {
                return Err("Move is outside the board".into());
            }
            let index = point.y * self.size + point.x;
            if self.cells[index].is_some() {
                return Err("That intersection is occupied".into());
            }
            self.cells[index] = Some(played.color);
            for neighbor in self.neighbors(index) {
                if self.cells[neighbor] == Some(played.color.other()) {
                    let (group, liberty) = self.group(neighbor);
                    if !liberty {
                        self.captures[played.color.index()] += group.len();
                        for stone in group {
                            self.cells[stone] = None;
                        }
                    }
                }
            }
            let (own, liberty) = self.group(index);
            if !liberty {
                if rules != "new-zealand" || own.len() == 1 {
                    return Err("Suicide is illegal under these rules".into());
                }
                self.captures[played.color.other().index()] += own.len();
                for stone in own {
                    self.cells[stone] = None;
                }
            }
            let repeated = if matches!(rules, "japanese" | "korean") {
                history.len() >= 2 && history[history.len() - 2].0 == self.cells
            } else {
                history.iter().any(|(cells, next)| {
                    *cells == self.cells && (rules != "aga" || *next == played.color.other())
                })
            };
            if repeated {
                return Err("Ko or superko forbids this recapture".into());
            }
        }
        self.next = played.color.other();
        self.move_number += 1;
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodePosition {
    pub id: NodeId,
    pub column: usize,
    pub row: usize,
}
pub fn normalize_rules(rules: &str) -> String {
    let lower = rules.to_ascii_lowercase();
    if lower.contains("jap") {
        "japanese"
    } else if lower.contains("kor") {
        "korean"
    } else if lower.contains("aga") {
        "aga"
    } else if lower.contains("zealand") || lower == "nz" {
        "new-zealand"
    } else {
        "chinese"
    }
    .into()
}
impl Document {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let text = match std::str::from_utf8(bytes) {
            Ok(text) => text.to_owned(),
            Err(_) => {
                let label = bytes.windows(3).position(|b| b == b"CA[").and_then(|i| {
                    bytes[i + 3..]
                        .iter()
                        .position(|b| *b == b']')
                        .map(|len| &bytes[i + 3..i + 3 + len])
                });
                let encoding = label
                    .and_then(encoding_rs::Encoding::for_label)
                    .unwrap_or(encoding_rs::WINDOWS_1252);
                let (text, _, errors) = encoding.decode(bytes);
                if errors {
                    return Err("Could not decode the SGF character encoding".into());
                }
                text.into_owned()
            }
        };
        let roots = sgf_parse::go::parse(&text)?;
        if roots.len() != 1 {
            return Err("Open a single-game SGF; collections are not supported yet".into());
        }
        let root = &roots[0];
        let size = match root.get_property("SZ") {
            Some(Prop::SZ((x, y))) if x == y => *x as usize,
            None => 19,
            _ => return Err("Only square Go boards are supported".into()),
        };
        if !(2..=19).contains(&size) {
            return Err("Supported board sizes are 2 through 19".into());
        }
        let mut doc = Self {
            size,
            ..Default::default()
        };
        fn add(
            doc: &mut Document,
            source: &SgfNode<Prop>,
            parent: Option<NodeId>,
        ) -> Result<NodeId> {
            let mut props = BTreeMap::new();
            let mut played = None;
            for prop in &source.properties {
                let key = prop.identifier();
                if matches!(prop, Prop::Invalid(_, _))
                    && matches!(
                        key.as_str(),
                        "B" | "W" | "SZ" | "KM" | "AB" | "AW" | "AE" | "PL"
                    )
                {
                    return Err(format!("Invalid SGF property {key}").into());
                }
                let mut values = unescape_values(&prop.to_string());
                if matches!(key.as_str(), "AB" | "AW" | "AE") {
                    values.sort();
                }
                if key == "B" || key == "W" {
                    if played.is_some() {
                        return Err("An SGF node cannot contain two moves".into());
                    }
                    played = Some(Move {
                        color: if key == "B" {
                            Color::Black
                        } else {
                            Color::White
                        },
                        point: Point::parse(values.first().ok_or("Missing move")?, doc.size)?,
                    });
                }
                props.insert(key, values);
            }
            let id = doc.nodes.len();
            doc.nodes.push(Node {
                parent,
                props,
                played,
                ..Default::default()
            });
            for child in &source.children {
                let child = add(doc, child, Some(id))?;
                doc.nodes[id].children.push(child);
            }
            Ok(id)
        }
        add(&mut doc, root, None)?;
        doc.nodes[0].props.insert("CA".into(), vec!["UTF-8".into()]);
        doc.rules = normalize_rules(doc.nodes[0].property("RU").unwrap_or("Chinese"));
        doc.komi = doc.nodes[0].property("KM").unwrap_or("7.5").parse()?;
        if !doc.komi.is_finite() || doc.komi.abs() > 400.0 || (doc.komi * 2.0).fract() != 0.0 {
            return Err("Komi must be a half-integer between -400 and 400".into());
        }
        let mut current = 0;
        loop {
            doc.mainline.push(current);
            if let Some(child) = doc.nodes[current].children.first() {
                current = *child;
            } else {
                break;
            }
        }
        for id in 0..doc.nodes.len() {
            doc.board(id)?;
        }
        Ok(doc)
    }
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
    fn replay(&self, node: NodeId) -> Result<(Board, BoardHistory)> {
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
        let mut board = Board {
            size: self.size,
            cells: vec![None; self.size * self.size],
            next: initial,
            move_number: 0,
            captures: [0, 0],
        };
        let mut history = vec![(board.cells.clone(), board.next)];
        for id in self.path(node)? {
            let n = &self.nodes[id];
            let mut setup = false;
            for (property, color) in [
                ("AE", None),
                ("AB", Some(Color::Black)),
                ("AW", Some(Color::White)),
            ] {
                if let Some(points) = n.props.get(property) {
                    for point in points {
                        let point =
                            Point::parse(point, self.size)?.ok_or("Setup stone cannot be pass")?;
                        board.cells[point.y * self.size + point.x] = color;
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
            if setup {
                history = vec![(board.cells.clone(), board.next)];
            }
            if let Some(played) = n.played {
                board.apply(played, &self.rules, &history)?;
                history.push((board.cells.clone(), board.next));
            }
        }
        Ok((board, history))
    }
    pub fn board(&self, node: NodeId) -> Result<Board> {
        Ok(self.replay(node)?.0)
    }
    pub fn append_move(&mut self, point: Option<Point>) -> Result<NodeId> {
        let (mut board, history) = self.replay(self.selected)?;
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
    pub fn to_sgf(&self) -> String {
        fn node(doc: &Document, id: NodeId) -> SgfNode<Prop> {
            let n = &doc.nodes[id];
            SgfNode::new(
                n.props
                    .iter()
                    .map(|(key, values)| Prop::new(key.clone(), values.clone()))
                    .collect(),
                n.children.iter().map(|child| node(doc, *child)).collect(),
                id == 0,
            )
        }
        node(self, 0).serialize()
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
            for (index, child) in doc.nodes[id].children.iter().enumerate() {
                let child_row = if index == 0 {
                    row
                } else {
                    let row = *next_row;
                    *next_row += 1;
                    row
                };
                visit(doc, *child, col + 1, child_row, positions, next_row);
            }
        }
        visit(self, 0, 0, 0, &mut positions, &mut next_row);
        positions
    }
}
fn unescape_values(property: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut chars = property.chars();
    while let Some(c) = chars.next() {
        if c != '[' {
            continue;
        }
        let mut value = String::new();
        while let Some(c) = chars.next() {
            if c == ']' {
                break;
            }
            if c == '\\' {
                if let Some(next) = chars.next()
                    && next != '\n'
                    && next != '\r'
                {
                    value.push(next);
                }
            } else {
                value.push(c);
            }
        }
        values.push(value);
    }
    values
}
