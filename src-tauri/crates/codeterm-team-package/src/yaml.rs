use std::collections::{BTreeMap, HashSet};

use yaml_rust2::parser::{Event, Parser};
use yaml_rust2::scanner::{Marker, TScalarStyle};

use crate::diagnostics::{DiagnosticCode, Sink};
use crate::Limits;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Mark {
    pub(crate) line: u32,
    pub(crate) column: u32,
}

impl Mark {
    pub(crate) const START: Self = Self { line: 1, column: 1 };

    fn from_marker(marker: &Marker) -> Self {
        Self {
            line: u32::try_from(marker.line()).unwrap_or(u32::MAX),
            column: u32::try_from(marker.col() + 1).unwrap_or(u32::MAX),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScalarStyle {
    Plain,
    Quoted,
    Block,
}

#[derive(Debug, Clone)]
pub(crate) enum NodeValue {
    Scalar { text: String, style: ScalarStyle },
    Seq(Vec<Node>),
    Map(Vec<(Key, Node)>),
}

#[derive(Debug, Clone)]
pub(crate) struct Key {
    pub(crate) text: String,
    pub(crate) mark: Mark,
}

#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub(crate) value: NodeValue,
    pub(crate) mark: Mark,
}

/// A loaded YAML source: leading comment block, root node and every node's position by path.
pub(crate) struct Loaded {
    pub(crate) header: Option<String>,
    pub(crate) root: Node,
    pub(crate) locations: Locations,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Locations(BTreeMap<String, Mark>);

impl Locations {
    pub(crate) fn at(&self, path: &str) -> Option<Mark> {
        let mut candidate = path;
        loop {
            if let Some(mark) = self.0.get(candidate) {
                return Some(*mark);
            }
            let cut = candidate.rfind(['.', '['])?;
            candidate = &candidate[..cut];
        }
    }

    fn index(&mut self, node: &Node, path: &str) {
        self.0.entry(path.to_string()).or_insert(node.mark);
        match &node.value {
            NodeValue::Scalar { .. } => {}
            NodeValue::Seq(items) => {
                for (index, item) in items.iter().enumerate() {
                    self.index(item, &format!("{path}[{index}]"));
                }
            }
            NodeValue::Map(entries) => {
                for (key, value) in entries {
                    let child = child_path(path, &key.text);
                    self.0.entry(child.clone()).or_insert(key.mark);
                    self.index(value, &child);
                }
            }
        }
    }
}

pub(crate) fn child_path(parent: &str, key: &str) -> String {
    if parent.is_empty() {
        key.to_string()
    } else {
        format!("{parent}.{key}")
    }
}

/// Normalizes line endings and drops a byte-order mark; refuses NUL and oversize input.
fn normalize(source: &str, limits: &Limits, sink: &mut Sink) -> Option<String> {
    if source.len() > limits.max_yaml_bytes {
        sink.error(
            DiagnosticCode::TooLarge,
            "",
            None,
            format!("team.yaml exceeds {} bytes", limits.max_yaml_bytes),
        );
        return None;
    }
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    if let Some(offset) = source.find('\0') {
        let line = source[..offset].matches('\n').count() + 1;
        sink.error(
            DiagnosticCode::ControlCharacter,
            "",
            Some(Mark {
                line: u32::try_from(line).unwrap_or(u32::MAX),
                column: 1,
            }),
            "NUL characters are not allowed",
        );
        return None;
    }
    Some(source.replace("\r\n", "\n").replace('\r', "\n"))
}

fn leading_comment(source: &str) -> Option<String> {
    let mut header = String::new();
    for line in source.split_inclusive('\n') {
        if !line.starts_with('#') {
            break;
        }
        header.push_str(line);
    }
    (!header.is_empty() && header.ends_with('\n')).then_some(header)
}

fn tab_indented_line(source: &str) -> Option<u32> {
    source
        .split('\n')
        .position(|line| line.starts_with('\t'))
        .map(|index| u32::try_from(index + 1).unwrap_or(u32::MAX))
}

enum Frame {
    Seq {
        path: String,
        mark: Mark,
        items: Vec<Node>,
    },
    Map {
        path: String,
        mark: Mark,
        entries: Vec<(Key, Node)>,
        seen: HashSet<String>,
        pending: Option<Key>,
    },
}

struct Builder<'a> {
    limits: &'a Limits,
    sink: &'a mut Sink,
    stack: Vec<Frame>,
    root: Option<Node>,
    nodes: usize,
}

impl Builder<'_> {
    fn check_extras(&mut self, anchor: usize, tagged: bool, mark: Mark) {
        if anchor != 0 {
            let path = self.next_path();
            self.sink.error(
                DiagnosticCode::YamlAnchor,
                &path,
                Some(mark),
                "YAML anchors are not allowed",
            );
        }
        if tagged {
            let path = self.next_path();
            self.sink.error(
                DiagnosticCode::YamlTag,
                &path,
                Some(mark),
                "YAML tags are not allowed",
            );
        }
    }

    fn count(&mut self, mark: Mark) -> bool {
        self.nodes += 1;
        if self.nodes > self.limits.max_nodes {
            self.sink.error(
                DiagnosticCode::TooManyItems,
                "",
                Some(mark),
                format!(
                    "the document holds more than {} nodes",
                    self.limits.max_nodes
                ),
            );
            return false;
        }
        true
    }

    fn open(&mut self, frame: Frame, mark: Mark) -> bool {
        if self.stack.len() >= self.limits.max_depth {
            self.sink.error(
                DiagnosticCode::TooDeep,
                "",
                Some(mark),
                format!("nesting exceeds {} levels", self.limits.max_depth),
            );
            return false;
        }
        self.stack.push(frame);
        true
    }

    /// The path of the value the parser delivers next.
    fn next_path(&self) -> String {
        match self.stack.last() {
            None => String::new(),
            Some(Frame::Seq { path, items, .. }) => format!("{path}[{}]", items.len()),
            Some(Frame::Map { path, pending, .. }) => match pending {
                Some(key) => child_path(path, &key.text),
                None => path.clone(),
            },
        }
    }

    fn close(&mut self) {
        let node = match self.stack.pop() {
            Some(Frame::Seq { mark, items, .. }) => Node {
                value: NodeValue::Seq(items),
                mark,
            },
            Some(Frame::Map { mark, entries, .. }) => Node {
                value: NodeValue::Map(entries),
                mark,
            },
            None => return,
        };
        self.place(node);
    }

    fn place(&mut self, node: Node) {
        let max_items = self.limits.max_items;
        match self.stack.last_mut() {
            None => self.root = Some(node),
            Some(Frame::Seq { items, .. }) => {
                if items.len() == max_items {
                    self.sink.error(
                        DiagnosticCode::TooManyItems,
                        "",
                        Some(node.mark),
                        format!("a list holds more than {max_items} items"),
                    );
                }
                items.push(node);
            }
            Some(Frame::Map {
                path,
                entries,
                seen,
                pending,
                ..
            }) => match pending.take() {
                Some(key) => {
                    if entries.len() == max_items {
                        self.sink.error(
                            DiagnosticCode::TooManyItems,
                            "",
                            Some(key.mark),
                            format!("a mapping holds more than {max_items} keys"),
                        );
                    }
                    if seen.insert(key.text.clone()) {
                        entries.push((key, node));
                    } else {
                        self.sink.error(
                            DiagnosticCode::DuplicateKey,
                            &child_path(path, &key.text),
                            Some(key.mark),
                            format!("duplicate key `{}`", key.text),
                        );
                    }
                }
                None => {
                    let text = match node.value {
                        NodeValue::Scalar { text, .. } => text,
                        _ => {
                            self.sink.error(
                                DiagnosticCode::ComplexKey,
                                "",
                                Some(node.mark),
                                "mapping keys must be plain strings",
                            );
                            String::new()
                        }
                    };
                    *pending = Some(Key {
                        text,
                        mark: node.mark,
                    });
                }
            },
        }
    }

    fn scalar(&mut self, text: String, style: TScalarStyle, mark: Mark) {
        if text.len() > self.limits.max_string_bytes {
            self.sink.error(
                DiagnosticCode::StringTooLong,
                "",
                Some(mark),
                format!("a string exceeds {} bytes", self.limits.max_string_bytes),
            );
        }
        let style = match style {
            TScalarStyle::Plain => ScalarStyle::Plain,
            TScalarStyle::SingleQuoted | TScalarStyle::DoubleQuoted => ScalarStyle::Quoted,
            TScalarStyle::Literal | TScalarStyle::Folded => ScalarStyle::Block,
        };
        self.place(Node {
            value: NodeValue::Scalar { text, style },
            mark,
        });
    }
}

fn syntax_error(sink: &mut Sink, info: &str, marker: &Marker) {
    let code = if info.contains("tab") {
        DiagnosticCode::TabIndentation
    } else {
        DiagnosticCode::YamlSyntax
    };
    sink.error(code, "", Some(Mark::from_marker(marker)), info.to_string());
}

/// Loads one YAML document strictly; returns `None` when nothing typed can be read.
pub(crate) fn load(source: &str, limits: &Limits, sink: &mut Sink) -> Option<Loaded> {
    let source = normalize(source, limits, sink)?;
    if let Some(line) = tab_indented_line(&source) {
        sink.error(
            DiagnosticCode::TabIndentation,
            "",
            Some(Mark { line, column: 1 }),
            "tabs cannot indent YAML",
        );
        return None;
    }
    let header = leading_comment(&source);
    let mut builder = Builder {
        limits,
        sink,
        stack: Vec::new(),
        root: None,
        nodes: 0,
    };
    let mut documents = 0;
    let mut parser = Parser::new_from_str(&source);
    loop {
        let (event, marker) = match parser.next_token() {
            Ok(token) => token,
            Err(error) => {
                syntax_error(builder.sink, error.info(), error.marker());
                return None;
            }
        };
        let mark = Mark::from_marker(&marker);
        match event {
            Event::StreamEnd => break,
            Event::DocumentStart => {
                documents += 1;
                if documents > 1 {
                    builder.sink.error(
                        DiagnosticCode::MultipleDocuments,
                        "",
                        Some(mark),
                        "a package holds exactly one YAML document",
                    );
                    return None;
                }
            }
            Event::Scalar(text, style, anchor, tag) => {
                if !builder.count(mark) {
                    return None;
                }
                builder.check_extras(anchor, tag.is_some(), mark);
                builder.scalar(text, style, mark);
            }
            Event::SequenceStart(anchor, tag) => {
                if !builder.count(mark) {
                    return None;
                }
                builder.check_extras(anchor, tag.is_some(), mark);
                let frame = Frame::Seq {
                    path: builder.next_path(),
                    mark,
                    items: Vec::new(),
                };
                if !builder.open(frame, mark) {
                    return None;
                }
            }
            Event::MappingStart(anchor, tag) => {
                if !builder.count(mark) {
                    return None;
                }
                builder.check_extras(anchor, tag.is_some(), mark);
                let frame = Frame::Map {
                    path: builder.next_path(),
                    mark,
                    entries: Vec::new(),
                    seen: HashSet::new(),
                    pending: None,
                };
                if !builder.open(frame, mark) {
                    return None;
                }
            }
            Event::SequenceEnd | Event::MappingEnd => builder.close(),
            Event::Alias(_) => {
                let path = builder.next_path();
                builder.sink.error(
                    DiagnosticCode::YamlAlias,
                    &path,
                    Some(mark),
                    "YAML aliases are not allowed",
                );
                builder.scalar(String::new(), TScalarStyle::Plain, mark);
            }
            Event::Nothing | Event::StreamStart | Event::DocumentEnd => {}
        }
    }
    let Some(root) = builder.root.take() else {
        builder.sink.error(
            DiagnosticCode::EmptyDocument,
            "",
            Some(Mark::START),
            "the package document is empty",
        );
        return None;
    };
    let mut locations = Locations::default();
    locations.index(&root, "");
    Some(Loaded {
        header,
        root,
        locations,
    })
}
