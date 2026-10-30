#[derive(Default)]
pub(super) struct HtmlBoundary {
    token: String,
    quote: Option<char>,
    truncated: bool,
    depth: usize,
    raw: String,
    probe: String,
    raw_closing: bool,
    outer: String,
}

impl HtmlBoundary {
    pub fn new(outer: &str) -> Self {
        Self {
            outer: outer.into(),
            depth: 1,
            ..Default::default()
        }
    }
    pub fn in_data(&self) -> bool {
        self.token.is_empty() && self.raw.is_empty() && self.probe.is_empty()
    }

    pub fn feed(&mut self, c: char) -> bool {
        if self.raw == "plaintext" {
            return false;
        }
        if !self.raw.is_empty() && !self.raw_closing {
            let expected = format!("</{}", self.raw);
            if self.probe.is_empty() {
                if c == '<' {
                    self.probe.push(c);
                }
                return false;
            }
            self.probe.push(c);
            let lower = self.probe.to_ascii_lowercase();
            if expected.starts_with(&lower) {
                return false;
            }
            if lower.starts_with(&expected)
                && matches!(c, '\t' | '\n' | '\x0c' | '\r' | ' ' | '/' | '>')
            {
                self.token = std::mem::take(&mut self.probe);
                self.raw_closing = true;
                if c == '>' {
                    self.end_token();
                }
            } else {
                self.probe = if c == '<' { "<".into() } else { String::new() };
            }
            return false;
        }
        if self.token.is_empty() {
            if c == '<' {
                self.token.push(c);
            }
            return false;
        }
        if self.token.len() < 1024 {
            self.token.push(c);
        } else {
            self.truncated = true;
        }
        if self.token == "<<" {
            self.token = "<".into();
            return false;
        }
        if self.token.chars().count() == 2
            && !(c.is_ascii_alphabetic() || matches!(c, '/' | '!' | '?'))
        {
            self.token.clear();
            return false;
        }
        if self.token.starts_with("<![CDATA[") {
            if self.token.ends_with("]]>") {
                self.token.clear();
            } else if self.token.len() > 12 {
                let tail: String = self
                    .token
                    .chars()
                    .rev()
                    .take(2)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                self.token = format!("<![CDATA[{tail}");
            }
            return false;
        }
        if self.token.starts_with("<!--") {
            if self.token == "<!-->" || self.token.ends_with("-->") || self.token.ends_with("--!>")
            {
                self.token.clear();
            } else if self.token.len() > 8 {
                let tail: String = self
                    .token
                    .chars()
                    .rev()
                    .take(3)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                self.token = format!("<!--{tail}");
            }
            return false;
        }
        if let Some(quote) = self.quote {
            if c == quote {
                self.quote = None;
            }
            return false;
        }
        if matches!(c, '\'' | '"') {
            self.quote = Some(c);
            return false;
        }
        c == '>' && self.end_token()
    }

    fn end_token(&mut self) -> bool {
        let token = std::mem::take(&mut self.token);
        let truncated = std::mem::take(&mut self.truncated);
        if self.raw_closing {
            self.raw_closing = false;
            self.raw.clear();
            return false;
        }
        if !truncated && token == format!("</{}>", self.outer) {
            self.depth -= 1;
            return self.depth == 0;
        }
        let name = tag_name(&token).unwrap_or_default().to_ascii_lowercase();
        if name == self.outer && !token.ends_with("/>") {
            self.depth += 1;
        }
        if matches!(
            name.as_str(),
            "script"
                | "style"
                | "title"
                | "textarea"
                | "xmp"
                | "iframe"
                | "noembed"
                | "noframes"
                | "noscript"
                | "plaintext"
        ) {
            self.raw = name;
        }
        false
    }
}

pub(super) fn tag_name(token: &str) -> Option<&str> {
    let rest = token.strip_prefix('<')?;
    if !rest.as_bytes().first()?.is_ascii_alphabetic() {
        return None;
    }
    let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))?;
    let next = rest.as_bytes()[end];
    (super::space(next) || matches!(next, b'/' | b'>')).then_some(&rest[..end])
}
