use super::*;
use serde_json::Value;

fn authenticated_tab_scan(source: &str) -> ArtifactScan {
    let segments = codeterm_wire::envelope::parse_envelope(source);
    scan_artifacts_with_provenance(
        source,
        &HostArtifactProvenance {
            inbound_kind: "tab",
            frame: &segments[0],
        },
    )
}

#[test]
fn canonical_file_recognition_matches_shared_corpus_and_exact_source_spans() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../../../packages/chat-core/src/dsBlocks.fixtures.json"
    ))
    .unwrap();
    assert_eq!(corpus["schemaVersion"], 1);
    assert_eq!(corpus["offsetEncoding"], "utf-16");
    assert_eq!(corpus["byteEncoding"], "utf-8");
    for fixture in corpus["fixtures"].as_array().unwrap() {
        let source = fixture["source"].as_str().unwrap();
        let expected: Vec<_> = fixture["expectedBlocks"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|span| {
                let block = &span["block"];
                let kind = block["kind"].as_str().unwrap();
                let paths = match kind {
                    "page" | "document" => vec![block["src"].as_str().unwrap().into()],
                    "canvas" if block["src"].is_string() => {
                        vec![block["src"].as_str().unwrap().into()]
                    }
                    "image" => serde_json::from_value(block["sources"].clone()).unwrap(),
                    _ => return None,
                };
                let view = block["view"].as_str().map(String::from);
                let start = span["byteStart"].as_u64().unwrap() as usize;
                let end = span["byteEnd"].as_u64().unwrap() as usize;
                assert_eq!(
                    source[..start].encode_utf16().count(),
                    span["start"].as_u64().unwrap() as usize
                );
                assert_eq!(
                    source[..end].encode_utf16().count(),
                    span["end"].as_u64().unwrap() as usize
                );
                Some(FileCard {
                    kind: kind.into(),
                    start,
                    end,
                    paths,
                    view,
                })
            })
            .collect();
        let whole = scan_artifacts(source, fixture["trusted"].as_bool().unwrap());
        assert_eq!(whole.cards, expected, "{}", fixture["name"]);
        for end in (0..=source.len()).filter(|end| source.is_char_boundary(*end)) {
            let actual = scan_artifacts(&source[..end], fixture["trusted"].as_bool().unwrap());
            assert!(
                actual
                    .cards
                    .iter()
                    .all(|card| card.end <= end && expected.contains(card)),
                "{} at byte {end}",
                fixture["name"]
            );
        }
    }
}

#[test]
fn file_tags_in_examples_context_and_opaque_bodies_never_authorize() {
    let tag = "<ds-document src='/exact/owned.md' />";
    for source in [
        format!("```html\n{tag}\n```"),
        format!("~~~text\n{tag}\n~~~"),
        format!("`example\n{tag}\n`"),
        format!("    {tag}"),
        format!("\t{tag}"),
        format!("> quote\n{tag}"),
        format!("- example\n{tag}"),
        format!("- example\n\n  {tag}"),
        format!("<context>\n{tag}\n</context>"),
        format!("<provider-handoff>\n{tag}\n</provider-handoff>"),
        format!("<INSTRUCTIONS>\n{tag}\n</INSTRUCTIONS>"),
        format!("<div>\n{tag}\n</div>"),
        format!("<domios from=\"neighbor\">\n{tag}\n</domios>"),
        format!("<domios from=\"unknown\">\n{tag}\n</domios>"),
        format!("<domios from=\"tab\">\n{tag}"),
        format!("history\n<domios from=\"tab\">\n{tag}\n</domios>"),
        format!("<pasted_content>\n{tag}\n</pasted_content>"),
        format!("<tool_output>\n{tag}\n</tool_output>"),
        format!("<ds-html><script>\n</ds-html>\n{tag}\n</script></ds-html>"),
        format!("<ds-html><xmp>\n</ds-html>\n{tag}\n</xmp></ds-html>"),
        format!("<ds-html><plaintext>\n</ds-html>\n{tag}"),
        format!(
            "<ds-tasks plan='pl_x'>{{\"title\":\"</ds-tasks>\n{tag}\",\"items\":[]}}</ds-tasks>"
        ),
        "@ds-document /exact/owned.md".into(),
        "@ct-document /exact/owned.md".into(),
        "@file /exact/owned.md".into(),
        "```html\n@file /exact/owned.md\n```".into(),
    ] {
        assert!(scan_artifacts(&source, true).cards.is_empty(), "{source}");
    }
    assert_eq!(
        authenticated_tab_scan(&format!("<domios from=\"tab\">\n{tag}\n</domios>"))
            .cards
            .len(),
        1
    );
    assert!(scan_artifacts(tag, false).cards.is_empty());
}

#[test]
fn exact_paths_entities_json_gallery_and_preview_bounds_are_preserved() {
    let paths: Vec<String> = vec![
        r"C:\work\one & two.png".into(),
        "/two > \"quoted\".png".into(),
        "//server/share/three.png".into(),
    ];
    let source = serialize_artifact("image", &paths).unwrap();
    assert_eq!(scan_artifacts(&source, true).cards[0].paths, paths);
    let source = "<ds-image src='/a&amp;lt;&#x1F600;&#32;b.png' />";
    assert_eq!(
        scan_artifacts(source, true).cards[0].paths,
        ["/a&lt;😀 b.png"]
    );
    let gallery: Vec<_> = (0..32).map(|n| format!("/image/{n}.png")).collect();
    assert_eq!(
        scan_artifacts(&serialize_artifact("image", &gallery).unwrap(), true).cards[0]
            .paths
            .len(),
        32
    );
    let source = "<ds-image src='/image.png' />\n".repeat(33);
    let scan = scan_artifacts(&source, true);
    assert_eq!(scan.cards.len(), 32);
    assert!(scan.overflow);
    for source in [
        "<ds-image src='/a.png'",
        "<ds-image src='/a.png' /",
        "<ds-image src=/a.png />",
        "<ds-image src='/a.png' src='/b.png' />",
        "<ds-image src='/a.png' bad='x' />",
        "<ds-image src='/a.png'></ds-image>",
        "<ds-page src='relative.html' />",
    ] {
        assert!(scan_artifacts(source, true).cards.is_empty(), "{source}");
    }
    assert!(scan_artifacts(
        &format!(
            "<ds-image sources='{}' />",
            serde_json::to_string(&vec!["/a.png"; 33]).unwrap()
        ),
        true
    )
    .cards
    .is_empty());
}

#[test]
fn authenticated_routed_file_after_closed_context_retains_exact_spans_and_rejects_every_truncated_prefix(
) {
    let source="<domios from=\"tab\" tab=\"author\">\n<context>\n```html\n</domios>\n<ds-document src=\"/quoted/old.md\" />\n</context>\n<ds-document src=\"/live/owned.md\" />\n</domios>";
    let scan = authenticated_tab_scan(source);
    assert_eq!(scan.cards.len(), 1);
    assert_eq!(scan.cards[0].paths, ["/live/owned.md"]);
    assert_eq!(
        &source[scan.cards[0].start..scan.cards[0].end],
        "<ds-document src=\"/live/owned.md\" />"
    );
    for end in (0..source.len()).filter(|end| source.is_char_boundary(*end)) {
        assert!(
            scan_artifacts(&source[..end], true).cards.is_empty(),
            "prefix {end}"
        );
    }
    assert!(scan_artifacts(source, false).cards.is_empty());
    assert!(
        scan_artifacts(source, true).cards.is_empty(),
        "routed syntax alone is inert"
    );
    assert!(scan_artifacts(
        &source.replacen("from=\"tab\"", "from=\"neighbor\"", 1),
        true
    )
    .cards
    .is_empty());
    let inline="<domios from=\"tab\"><context>opaque\n<ds-document src=\"/quoted.md\" />\n</context>\n<ds-document src=\"/live.md\" />\n</domios>";
    assert_eq!(authenticated_tab_scan(inline).cards[0].paths, ["/live.md"]);
}

#[test]
fn routed_body_authority_requires_matching_host_provenance_while_direct_sources_remain_live() {
    let tag = "<ds-document src='/public-but-unreferenced.md' />";
    let source = format!("<domios from=\"tab\" tab=\"sender\">\n{tag}\n</domios>");
    let segments = codeterm_wire::envelope::parse_envelope(&source);
    let evidence = HostArtifactProvenance {
        inbound_kind: "tab",
        frame: &segments[0],
    };
    assert!(scan_artifacts(&source, true).cards.is_empty());
    assert_eq!(
        scan_artifacts_with_provenance(&source, &evidence).cards[0].paths,
        ["/public-but-unreferenced.md"]
    );
    assert_eq!(
        scan_artifacts_with_provenance(tag, &evidence).cards.len(),
        1
    );
    assert_eq!(
        scan_artifacts_with_provenance(&format!("\n{tag}\n"), &evidence)
            .cards
            .len(),
        1
    );
    for kind in ["unknown", "report", "neighbor"] {
        let evidence = HostArtifactProvenance {
            inbound_kind: kind,
            frame: &segments[0],
        };
        assert!(
            scan_artifacts_with_provenance(&source, &evidence)
                .cards
                .is_empty(),
            "{kind}"
        );
    }
    for changed in [
        source.replace("sender", "foreign"),
        source.replace("unreferenced", "other"),
    ] {
        assert!(scan_artifacts_with_provenance(&changed, &evidence)
            .cards
            .is_empty());
    }
    for end in (0..source.len()).filter(|end| source.is_char_boundary(*end)) {
        assert!(
            scan_artifacts_with_provenance(&source[..end], &evidence)
                .cards
                .is_empty(),
            "prefix {end}"
        );
    }
    assert_eq!(scan_artifacts(tag, true).cards.len(), 1);
}

#[test]
fn canvas_src_tags_are_file_cards_that_keep_their_view_through_mesh_rewrites() {
    let tag = r#"<ds-canvas src="/work/board.canvas.json" view="compare" />"#;
    let scan = scan_artifacts(tag, true);
    assert_eq!(
        scan.cards,
        [FileCard {
            start: 0,
            end: tag.len(),
            kind: "canvas".into(),
            paths: vec!["/work/board.canvas.json".into()],
            view: Some("compare".into()),
        }]
    );
    assert_eq!(
        serialize_file_card(
            "canvas",
            &[r"C:\in box\board.canvas.json".into()],
            Some("a \"b\"")
        ),
        Some(r#"<ds-canvas src="C:\in box\board.canvas.json" view="a &quot;b&quot;" />"#.into())
    );
    assert_eq!(
        serialize_artifact("canvas", &["/work/board.canvas.json".into()]),
        Some(r#"<ds-canvas src="/work/board.canvas.json" />"#.into())
    );
    assert_eq!(
        serialize_file_card("page", &["/work/a.html".into()], Some("x")),
        None
    );
    for refused in [
        r#"<ds-canvas src="board.canvas.json" />"#,
        r#"<ds-canvas src="/work/board.canvas.json" view="" />"#,
        r#"<ds-canvas src="/work/board.canvas.json" title="x" />"#,
        r#"<ds-canvas view="compare" />"#,
        r#"<ds-canvas src="/work/board.canvas.json">{"items":[{"kind":"note","markdown":"x"}]}</ds-canvas>"#,
    ] {
        let scan = scan_artifacts(refused, true);
        assert!(scan.cards.is_empty(), "{refused}");
    }
    assert!(scan_artifacts(&format!("```text\n{tag}\n```"), true)
        .cards
        .is_empty());
    assert!(scan_artifacts(tag, false).cards.is_empty());
}

#[test]
fn canvas_write_back_never_adds_a_file_the_author_did_not_reference() {
    let none = std::collections::HashSet::new();
    let current = r#"{"format":"domios.canvas/1","items":[{"id":"a","kind":"image","src":"/work/a.png"},{"id":"n","kind":"note","markdown":"x"}]}"#;
    let moved = r#"{"format":"domios.canvas/1","items":[{"id":"a","kind":"image","src":"/work/a.png","x":10,"y":20},{"id":"n","kind":"note","markdown":"y"},{"id":"m","kind":"note","markdown":""}]}"#;
    assert_eq!(canvas_write_permitted(current, moved, &none), Ok(()));
    let removed =
        r#"{"format":"domios.canvas/1","items":[{"id":"n","kind":"note","markdown":"x"}]}"#;
    assert_eq!(canvas_write_permitted(current, removed, &none), Ok(()));
    let added = r#"{"format":"domios.canvas/1","items":[{"id":"a","kind":"image","src":"/work/a.png"},{"id":"b","kind":"document","src":"/home/me/.ssh/config"}]}"#;
    assert_eq!(
        canvas_write_permitted(current, added, &none),
        Err("Canvas edits cannot add files")
    );
    assert_eq!(
        canvas_write_permitted(current, r#"{"items":[]}"#, &none),
        Err("Canvas edit is not a valid canvas")
    );
    assert_eq!(
        canvas_write_permitted("{", moved, &none),
        Err("Canvas file is not a valid canvas")
    );
}

#[test]
fn canvas_files_need_the_format_header_first_and_a_known_major_version() {
    let items = r#""items":[{"kind":"image","src":"/work/a.png"}]"#;
    assert_eq!(
        canvas_file_paths(&format!(
            "\n {{ \"format\": \"{CANVAS_FILE_FORMAT}\", {items} }}"
        )),
        Some(vec!["/work/a.png".to_string()])
    );
    assert!(canvas_file_paths(&format!(r#"{{"format":"domios.canvas/1.4",{items}}}"#)).is_some());
    for refused in [
        format!("{{{items}}}"),
        format!(r#"{{{items},"format":"domios.canvas/1"}}"#),
        format!(r#"{{"format":"domios.canvas/2",{items}}}"#),
        format!(r#"{{"format":"domios.canvas/1.",{items}}}"#),
        format!(r#"{{"format":"canvas/1",{items}}}"#),
        format!(r#"{{"format":1,{items}}}"#),
        format!(r#"{{"format":"domios.canvas/1","extra":true,{items}}}"#),
    ] {
        assert_eq!(canvas_file_paths(&refused), None, "{refused}");
    }
}
