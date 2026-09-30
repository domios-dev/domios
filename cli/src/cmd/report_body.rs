/// Every way a report body can arrive, already read into memory by the caller.
#[derive(Default)]
pub struct ReportBodyInput<'a> {
    /// Contents of the `--from-pad` file.
    pub pad: Option<&'a str>,
    /// `--text` value.
    pub text: Option<&'a str>,
    /// Positional message.
    pub message: Option<&'a str>,
    /// `--doc` path plus its contents.
    pub doc: Option<(&'a str, &'a str)>,
}

/// Compose the body the parent actually receives: staged pad content wins over
/// `--text`, which wins over the positional, and `--doc` appends under its path.
pub fn resolve_report_body(input: ReportBodyInput<'_>) -> String {
    let mut body = input
        .pad
        .or(input.text)
        .or(input.message)
        .unwrap_or_default()
        .to_string();
    if let Some((path, doc)) = input.doc {
        if !body.is_empty() {
            body.push_str("\n\n");
        }
        body.push_str(&format!("doc: {path}\n\n{doc}"));
    }
    body
}

/// Read every report-body source the CLI accepts and compose the delivered
/// body. Exits with the caller-facing error when a staged source is unreadable.
pub fn read_report_body(
    message: Option<String>,
    text: Option<String>,
    from_pad: Option<String>,
    doc: Option<String>,
) -> String {
    let pad_body = from_pad.map(|path| {
        crate::cmd::payload_source::read_from_pad(&path).unwrap_or_else(|e| {
            crate::cmd::print::fail(1, e.to_string());
        })
    });
    if pad_body.is_none() {
        crate::cmd::payload_source::refuse_inline_report_body(
            text.as_deref().or(message.as_deref()),
        );
    }
    let doc_body = doc.map(|path| {
        let body = std::fs::read_to_string(&path).unwrap_or_else(|e| {
            crate::cmd::print::fail(1, format!("failed to read --doc {}: {}", path, e));
        });
        (path, body)
    });
    resolve_report_body(ReportBodyInput {
        pad: pad_body.as_deref(),
        text: text.as_deref(),
        message: message.as_deref(),
        doc: doc_body.as_ref().map(|(p, b)| (p.as_str(), b.as_str())),
    })
}

#[cfg(test)]
mod tests;
