use std::collections::BTreeMap;
use yaml_rust2::parser::{Event, Parser};

#[derive(Default)]
pub(super) struct Positions {
    pub locations: BTreeMap<String, (usize, usize)>,
    pub duplicates: Vec<(String, usize, usize)>,
}
struct Frame {
    path: String,
    sequence: bool,
    index: usize,
    key: Option<String>,
}

impl Positions {
    pub fn parse(source: &str) -> Self {
        let mut positions = Self::default();
        let mut stack: Vec<Frame> = Vec::new();
        let mut parser = Parser::new_from_str(source);
        loop {
            let Ok((event, mark)) = parser.next_token() else {
                break;
            };
            if matches!(event, Event::StreamEnd) {
                break;
            }
            match event {
                Event::MappingEnd | Event::SequenceEnd => {
                    stack.pop();
                }
                Event::Scalar(ref key, _, _, _)
                    if stack.last().is_some_and(|f| !f.sequence && f.key.is_none()) =>
                {
                    if let Some(frame) = stack.last_mut() {
                        let path = join(&frame.path, key);
                        if positions
                            .locations
                            .insert(path.clone(), (mark.line(), mark.col() + 1))
                            .is_some()
                        {
                            positions
                                .duplicates
                                .push((path, mark.line(), mark.col() + 1));
                        }
                        frame.key = Some(key.clone());
                    }
                }
                Event::Scalar(..)
                | Event::MappingStart(..)
                | Event::SequenceStart(..)
                | Event::Alias(..) => {
                    let path = if let Some(frame) = stack.last_mut() {
                        if frame.sequence {
                            let path = format!("{}[{}]", frame.path, frame.index);
                            frame.index += 1;
                            path
                        } else {
                            join(&frame.path, &frame.key.take().unwrap_or_default())
                        }
                    } else {
                        String::new()
                    };
                    positions
                        .locations
                        .entry(path.clone())
                        .or_insert((mark.line(), mark.col() + 1));
                    if matches!(event, Event::MappingStart(..) | Event::SequenceStart(..)) {
                        stack.push(Frame {
                            path,
                            sequence: matches!(event, Event::SequenceStart(..)),
                            index: 0,
                            key: None,
                        });
                    }
                }
                _ => {}
            }
        }
        positions
    }
    pub fn at(&self, path: &str) -> (usize, usize) {
        let mut path = path.to_string();
        loop {
            if let Some(position) = self.locations.get(&path) {
                return *position;
            }
            let Some(end) = path.rfind(['.', '[']) else {
                return (1, 1);
            };
            path.truncate(end);
        }
    }
}
fn join(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.into()
    } else {
        format!("{prefix}.{key}")
    }
}
