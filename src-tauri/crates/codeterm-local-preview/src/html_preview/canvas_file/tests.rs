use super::*;
use crate::html_preview::{preview_test_root, read_local_preview_file_guarded};

const BOARD: &str = r#"{"format":"domios.canvas/1","items":[{"id":"a","kind":"image","src":"/work/a.png"},{"id":"n","kind":"note","markdown":"agent"}]}"#;
const MOVED: &str = r#"{"format":"domios.canvas/1","items":[{"id":"a","kind":"image","src":"/work/a.png","x":40,"y":0},{"id":"n","kind":"note","markdown":"agent"}]}"#;

fn board(dir: &Path, name: &str, text: &str) -> String {
    let path = preview_test_root(dir).join(name);
    std::fs::write(&path, text).unwrap();
    path.to_string_lossy().into_owned()
}

fn open(_: &Path, _: Option<&Path>) -> Result<(), String> {
    Ok(())
}

fn leftovers(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".tmp"))
        .collect()
}

#[test]
fn canvas_files_read_as_bounded_json_only_with_the_canvas_extension() {
    let temp = tempfile::tempdir().unwrap();
    let path = board(temp.path(), "board.canvas.json", BOARD);
    let file = read_local_preview_file_guarded(&path, "canvas", open).unwrap();
    assert_eq!(
        (file.data.as_str(), file.media_type.as_str()),
        (BOARD, "application/json")
    );
    let plain = board(temp.path(), "board.json", BOARD);
    assert_eq!(
        read_local_preview_file_guarded(&plain, "canvas", open).unwrap_err(),
        "Canvas preview source must use .canvas.json"
    );
    let large = board(
        temp.path(),
        "large.canvas.json",
        &" ".repeat(CANVAS_FILE_LIMIT as usize + 1),
    );
    assert!(read_local_preview_file_guarded(&large, "canvas", open)
        .unwrap_err()
        .starts_with("Local preview source exceeds"));
    assert!(!is_canvas_file(Path::new("/work/.canvas.json")));
    assert!(is_canvas_file(Path::new("/work/Board.Canvas.JSON")));
}

#[test]
fn write_replaces_an_unchanged_canvas_and_leaves_no_temp_file() {
    let temp = tempfile::tempdir().unwrap();
    let path = board(temp.path(), "board.canvas.json", BOARD);
    assert_eq!(
        write_canvas_file(&path, open, BOARD, MOVED),
        Ok(CanvasWrite::Written)
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), MOVED);
    assert!(leftovers(temp.path()).is_empty());
}

#[test]
fn a_canvas_changed_underneath_returns_its_text_and_keeps_the_agent_change() {
    let temp = tempfile::tempdir().unwrap();
    let agent = BOARD.replace("agent", "agent edit");
    let path = board(temp.path(), "board.canvas.json", &agent);
    assert_eq!(
        write_canvas_file(&path, open, BOARD, MOVED),
        Ok(CanvasWrite::Conflict(agent.clone()))
    );
    assert_eq!(std::fs::read_to_string(&path).unwrap(), agent);
    assert!(leftovers(temp.path()).is_empty());
}

#[test]
fn write_refuses_guarded_paths_added_files_invalid_documents_and_other_extensions() {
    let temp = tempfile::tempdir().unwrap();
    let path = board(temp.path(), "board.canvas.json", BOARD);
    let refuse = |_: &Path, _: Option<&Path>| Err::<(), _>("Preview path is private".to_string());
    assert_eq!(
        write_canvas_file(&path, refuse, BOARD, MOVED).unwrap_err(),
        "Preview path is private"
    );
    let added = MOVED.replace(
        r#"{"id":"n""#,
        r#"{"id":"b","kind":"document","src":"/etc/passwd"},{"id":"n""#,
    );
    assert_eq!(
        write_canvas_file(&path, open, BOARD, &added).unwrap_err(),
        "Canvas edits cannot add files"
    );
    assert_eq!(
        write_canvas_file(&path, open, BOARD, "{").unwrap_err(),
        "Canvas edit is not a valid canvas"
    );
    let plain = board(temp.path(), "board.json", BOARD);
    assert!(write_canvas_file(&plain, open, BOARD, MOVED).is_err());
    assert!(probe_canvas_file(&plain, open).is_err());
    assert!(probe_canvas_file(&path, open).is_ok());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), BOARD);
    assert!(leftovers(temp.path()).is_empty());
}
