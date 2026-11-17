use super::*;

struct Silent;

impl UiEvents for Silent {
    fn emit(&self, _: &str, _: serde_json::Value) {}
    fn emit_to(&self, _: &str, _: &str, _: serde_json::Value) {}
}

#[test]
fn a_full_registry_refuses_a_new_canvas_instead_of_evicting_an_open_viewer() {
    let temp = tempfile::tempdir().unwrap();
    let registry = CanvasWatches::new(2);
    let ui: Arc<dyn UiEvents> = Arc::new(Silent);
    let canvas = |name: &str| {
        let path = temp.path().join(name);
        std::fs::write(&path, "{}").unwrap();
        let canonical = path.canonicalize().unwrap();
        (path.to_string_lossy().into_owned(), canonical)
    };
    let (first, first_canonical) = canvas("first.canvas.json");
    let (second, second_canonical) = canvas("second.canvas.json");
    let (third, third_canonical) = canvas("third.canvas.json");
    let watch = |path: &str, canonical: &Path| registry.watch(ui.clone(), "tab", path, canonical);
    assert_eq!(watch(&first, &first_canonical), Ok(true));
    assert_eq!(watch(&second, &second_canonical), Ok(true));
    assert_eq!(
        watch(&first, &first_canonical),
        Ok(true),
        "another viewer of a watched canvas shares its slot"
    );
    assert_eq!(
        watch(&third, &third_canonical),
        Ok(false),
        "the cap refuses"
    );
    let held = |path: &str| {
        registry
            .entries
            .lock()
            .iter()
            .find(|watch| watch.key == key(path))
            .map(|watch| watch.holders)
    };
    assert_eq!(
        (held(&first), held(&second)),
        (Some(2), Some(1)),
        "no open viewer lost its watch"
    );
    registry.release("tab", &first);
    assert_eq!(
        watch(&third, &third_canonical),
        Ok(false),
        "a shared slot stays held"
    );
    registry.release("tab", &second);
    assert_eq!(
        watch(&third, &third_canonical),
        Ok(true),
        "a released slot is free again"
    );
    assert_eq!(held(&first), Some(1));
}
