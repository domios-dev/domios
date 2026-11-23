use std::path::{Component, Path, PathBuf};

use codeterm_local_preview::{
    artifact_directive::{canvas_file_paths, scan_artifacts, serialize_file_card},
    html_preview::{is_canvas_file, write_pad_inbox, CANVAS_FILE_LIMIT},
    PreviewPolicy,
};
use serde::Deserialize;

use super::{agent_pad_root, open_source, MAX_FILES};
use crate::service::ServiceContext;

#[derive(Debug, Default, Deserialize)]
pub struct PadAttachments {
    #[serde(default)]
    pub attach: Vec<String>,
    #[serde(default)]
    pub copy: bool,
}

impl PadAttachments {
    pub fn is_empty(&self) -> bool {
        self.attach.is_empty() && !self.copy
    }
}

fn source_path(pad: &Path, relative: &str) -> Result<String, String> {
    let path = Path::new(relative);
    if relative.is_empty()
        || relative != relative.trim()
        || !path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
        || relative
            .split(['/', '\\'])
            .any(|part| matches!(part, "." | ".."))
    {
        return Err("Pad attachment must be a relative file in the sender's pad".into());
    }
    path_text(&pad.join(path))
}

fn path_text(path: &Path) -> Result<String, String> {
    codeterm_paths::simplified_path(path)
        .to_str()
        .map(str::to_owned)
        .ok_or_else(|| "Pad attachment path is not valid Unicode".into())
}

fn kind(path: &Path) -> &'static str {
    if is_canvas_file(path) {
        return "canvas";
    }
    match extension(path).as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" => "image",
        "html" | "htm" => "page",
        _ => "document",
    }
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

pub(super) fn prepare(
    svc: &ServiceContext,
    recipient: &str,
    pad: &Path,
    policy: &PreviewPolicy,
    body: &str,
    options: &PadAttachments,
) -> Result<PreparedAttachments, String> {
    if options.is_empty() {
        return Ok(PreparedAttachments::default());
    }
    if options.attach.is_empty() {
        return Err("Copy requires at least one attached pad file".into());
    }
    let scan = scan_artifacts(body, true);
    let count = scan
        .cards
        .iter()
        .map(|card| card.paths.len())
        .sum::<usize>();
    if scan.overflow || options.attach.len() > MAX_FILES - count {
        return Err("A message may contain at most 32 file previews".into());
    }
    let recipient_pad = if options.copy {
        Some(agent_pad_root(svc, recipient).ok_or("Recipient pad is unavailable")?)
    } else {
        None
    };
    let mut snapshot = recipient_pad.map(Snapshot::new);
    let mut cards = Vec::new();
    for relative in &options.attach {
        let source = source_path(pad, relative)?;
        let kind = kind(Path::new(&source));
        let rendered = if let Some(snapshot) = &mut snapshot {
            snapshot.copy(pad, policy, &source, kind == "canvas")?
        } else {
            open_source(pad, policy, &source, None)?;
            source
        };
        cards.push(serialize_file_card(kind, &[rendered], None).ok_or("Invalid pad attachment")?);
    }
    let copied_items = snapshot
        .as_ref()
        .map_or(0, |snapshot| snapshot.files.len() - options.attach.len());
    Ok(PreparedAttachments {
        body: Some(format!("{}\n\n{body}", cards.join("\n"))),
        copied_items,
        snapshot,
    })
}

#[derive(Default)]
pub(super) struct PreparedAttachments {
    pub(super) body: Option<String>,
    pub(super) copied_items: usize,
    snapshot: Option<Snapshot>,
}

impl PreparedAttachments {
    pub(super) fn write_snapshot(&self) -> Result<(), String> {
        if let Some(snapshot) = &self.snapshot {
            write_pad_inbox(&snapshot.pad, &snapshot.files)?;
        }
        Ok(())
    }
}

struct Snapshot {
    pad: PathBuf,
    id: uuid::Uuid,
    remaining: u64,
    files: Vec<(String, Vec<u8>)>,
}

impl Snapshot {
    fn new(pad: PathBuf) -> Self {
        Self {
            pad,
            id: uuid::Uuid::new_v4(),
            remaining: crate::service::mesh_attachment::MESH_ATTACHMENT_MAX_BYTES,
            files: Vec::new(),
        }
    }

    fn copy(
        &mut self,
        source_pad: &Path,
        policy: &PreviewPolicy,
        raw: &str,
        canvas: bool,
    ) -> Result<String, String> {
        if self.files.len() >= MAX_FILES {
            return Err("A message may contain at most 32 copied pad files".into());
        }
        let limit = if canvas {
            self.remaining.min(CANVAS_FILE_LIMIT)
        } else {
            self.remaining
        };
        let (file, bytes) = open_source(source_pad, policy, raw, Some(limit))?;
        let bytes = bytes.ok_or("Pad snapshot has no source")?;
        let bytes = if canvas {
            self.rewrite_canvas(source_pad, policy, &bytes)?
        } else {
            bytes
        };
        self.remaining = self
            .remaining
            .checked_sub(bytes.len() as u64)
            .ok_or("Pad snapshots exceed the attachment size limit")?;
        let extension = if canvas {
            ".canvas.json".into()
        } else {
            let extension = extension(&file);
            if extension.len() <= 16
                && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
                && !extension.is_empty()
            {
                format!(".{extension}")
            } else {
                String::new()
            }
        };
        let name = format!("share-{}-{}{extension}", self.id.simple(), self.files.len());
        let target = path_text(&self.pad.join("inbox").join(&name))?;
        self.files.push((name, bytes));
        Ok(target)
    }

    fn rewrite_canvas(
        &mut self,
        source_pad: &Path,
        policy: &PreviewPolicy,
        bytes: &[u8],
    ) -> Result<Vec<u8>, String> {
        let text = std::str::from_utf8(bytes).map_err(|error| error.to_string())?;
        let children = canvas_file_paths(text).ok_or("Pad snapshot canvas is invalid")?;
        if self.files.len() + children.len() >= MAX_FILES {
            return Err("A message may contain at most 32 copied pad files".into());
        }
        let mut board: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(text).map_err(|error| error.to_string())?;
        let format = board
            .remove("format")
            .ok_or("Pad snapshot canvas format is missing")?;
        let items = board
            .get_mut("items")
            .and_then(serde_json::Value::as_array_mut)
            .ok_or("Pad snapshot canvas items are missing")?;
        for item in items {
            if let Some(source) = item.get_mut("src") {
                let raw = source.as_str().ok_or("Invalid canvas item source")?;
                *source = self.copy(source_pad, policy, raw, false)?.into();
            }
        }
        let fields = serde_json::to_string(&board).map_err(|error| error.to_string())?;
        let board = format!("{{\"format\":{format},{}", &fields[1..]);
        if canvas_file_paths(&board).is_none() {
            return Err("Pad snapshot canvas exceeds its preview limits".into());
        }
        Ok(board.into_bytes())
    }
}
