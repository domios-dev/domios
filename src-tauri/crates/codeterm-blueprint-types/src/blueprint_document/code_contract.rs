use super::{Code, Language, Limits, Node};

pub fn has_main(code: &Code) -> bool {
    let Some(source) = &code.src else {
        return false;
    };
    let pattern = match code.lang {
        Language::Python => {
            r"(?m)^def[ \t]+main[ \t]*\([ \t]*[A-Za-z_][A-Za-z_0-9]*[ \t]*(?::[^)]+)?\)[ \t]*(?:->[^:]+)?[ \t]*:"
        }
        Language::Ts => r"(?m)^export[ \t]+default[ \t]+(?:async[ \t]+)?function[ \t]+main[ \t]*\(",
        Language::Shell => return false,
    };
    regex::Regex::new(pattern).is_ok_and(|pattern| pattern.is_match(source))
}

pub(super) fn errors(node: &Node, path: &str) -> Vec<(String, String)> {
    let mut errors = Vec::new();
    if let Some(code) = &node.code {
        if code.src.as_ref().is_some_and(|src| src.len() > 16384) {
            errors.push((
                format!("{path}.code.src"),
                "Code body exceeds 16 KiB".into(),
            ));
        }
        if code.src.is_some() && !has_main(code) {
            errors.push((format!("{path}.code.src"), "Declare main(value): Python def main(value), TS export default function main(value)".into()));
        }
        limits(
            &code.limits,
            node,
            &format!("{path}.code.limits"),
            &mut errors,
        );
    }
    if let Some(budget) = &node.limits {
        limits(budget, node, &format!("{path}.limits"), &mut errors);
    }
    errors
}

fn limits(limits: &Limits, node: &Node, path: &str, errors: &mut Vec<(String, String)>) {
    if limits.mem_mb.is_some() && node.code.is_none() {
        errors.push((
            format!("{path}.mem_mb"),
            "unsupported: memory budget on this node".into(),
        ));
    }
    if limits.output_bytes.is_some() && !matches!(node.kind(), "code" | "command" | "github") {
        errors.push((
            format!("{path}.output_bytes"),
            "unsupported: output budget on this node".into(),
        ));
    }
    if limits.wall_ms == Some(0) || limits.output_bytes == Some(0) || limits.mem_mb == Some(0) {
        errors.push((path.into(), "Budgets must be positive".into()));
    }
}

pub fn body_sha256(code: &Code) -> Option<String> {
    use sha2::{Digest, Sha256};
    code.src
        .as_ref()
        .map(|source| format!("{:x}", Sha256::digest(source.as_bytes())))
}
