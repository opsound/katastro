//! SGF decoding, import normalization, and export.
use super::*;
use sgf_parse::{SgfNode, SgfProp, go::Prop};
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
        Self::parse_with_source_komi(bytes).map(|(doc, _)| doc)
    }
    /// Keep the source komi available for recovering reviews saved by older imports.
    pub(crate) fn parse_with_source_komi(bytes: &[u8]) -> Result<(Self, f64)> {
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
        let source_komi = doc.komi;
        // Fox can emit duplicate AP properties in either order. Inspect parsed
        // root properties so comments and other applications cannot trigger this.
        let fox = root.properties.iter().any(|prop| {
            prop.identifier() == "AP"
                && unescape_values(&prop.to_string()).iter().any(|value| {
                    value
                        .split(':')
                        .next()
                        .is_some_and(|name| name.eq_ignore_ascii_case("foxwq"))
                })
        });
        if fox {
            // Known Fox encodings only; retain explicit zero/handicap komi and
            // values already expressed in points (including reimported exports).
            doc.komi = match (doc.rules.as_str(), source_komi) {
                ("chinese", 375.0) => 7.5,
                ("chinese", 325.0) | ("japanese", 650.0) => 6.5,
                _ => source_komi,
            };
            if doc.komi != source_komi {
                doc.nodes[0]
                    .props
                    .insert("KM".into(), vec![doc.komi.to_string()]);
            }
        }
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
        doc.validate_tree()?;
        Ok((doc, source_komi))
    }
    pub fn to_sgf(&self) -> String {
        fn node(doc: &Document, id: NodeId) -> SgfNode<Prop> {
            let n = &doc.nodes[id];
            SgfNode::new(
                n.props
                    .iter()
                    .map(|(key, values)| Prop::new(key.clone(), values.clone()))
                    .collect(),
                doc.ordered_children(id)
                    .map(|child| node(doc, child))
                    .collect(),
                id == 0,
            )
        }
        node(self, 0).serialize()
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
