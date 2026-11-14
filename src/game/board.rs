//! Go rules and connected chains; shared by replay and board overlays.
use super::{Color, Move, Point};
use crate::Result;
use std::collections::HashSet;
pub(super) type BoardHistory = Vec<(Vec<Option<Color>>, Color)>;
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
    pub(crate) fn group(&self, start: usize) -> (Vec<usize>, bool) {
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
    pub(super) fn apply(
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
                    *cells == self.cells
                        && (!matches!(rules, "aga" | "new-zealand")
                            || *next == played.color.other())
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
