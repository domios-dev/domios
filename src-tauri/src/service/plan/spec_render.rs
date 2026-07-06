use crate::plan::{Section, Spec};

/// Render a spec's section tree as a markdown document (headings + bodies).
pub fn spec_markdown(spec: &Spec) -> String {
    let mut out = String::new();
    render_sections(&spec.sections, 1, &mut out);
    out
}

fn render_sections(sections: &[Section], depth: usize, out: &mut String) {
    for section in sections {
        let level = "#".repeat(depth.min(6));
        out.push_str(&format!("{level} {}\n\n", section.title));
        if !section.body.is_empty() {
            out.push_str(&section.body);
            out.push_str("\n\n");
        }
        render_sections(&section.subsections, depth + 1, out);
    }
}
