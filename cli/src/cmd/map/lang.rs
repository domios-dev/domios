use std::path::Path;
use tree_sitter::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    Rust,
    TypeScript,
    Tsx,
    JavaScript,
    Python,
    Go,
    Java,
}

impl Lang {
    pub fn name(self) -> &'static str {
        match self {
            Lang::Rust => "rust",
            Lang::TypeScript => "typescript",
            Lang::Tsx => "tsx",
            Lang::JavaScript => "javascript",
            Lang::Python => "python",
            Lang::Go => "go",
            Lang::Java => "java",
        }
    }

    pub fn from_path(path: &Path) -> Option<Lang> {
        let ext = path.extension()?.to_str()?;
        Lang::from_extension(ext)
    }

    pub fn from_extension(ext: &str) -> Option<Lang> {
        match ext.to_ascii_lowercase().as_str() {
            "rs" => Some(Lang::Rust),
            "ts" | "mts" | "cts" => Some(Lang::TypeScript),
            "tsx" => Some(Lang::Tsx),
            "js" | "mjs" | "cjs" | "jsx" => Some(Lang::JavaScript),
            "py" | "pyi" => Some(Lang::Python),
            "go" => Some(Lang::Go),
            "java" => Some(Lang::Java),
            _ => None,
        }
    }

    pub fn ts_language(self) -> Language {
        match self {
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
            Lang::Go => tree_sitter_go::LANGUAGE.into(),
            Lang::Java => tree_sitter_java::LANGUAGE.into(),
        }
    }

    pub fn tags_query(self) -> &'static str {
        match self {
            Lang::Rust => include_str!("queries/rust-tags.scm"),
            Lang::TypeScript | Lang::Tsx => include_str!("queries/typescript-tags.scm"),
            Lang::JavaScript => include_str!("queries/javascript-tags.scm"),
            Lang::Python => include_str!("queries/python-tags.scm"),
            Lang::Go => include_str!("queries/go-tags.scm"),
            Lang::Java => include_str!("queries/java-tags.scm"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn detects_extensions() {
        assert_eq!(Lang::from_path(&PathBuf::from("foo.rs")), Some(Lang::Rust));
        assert_eq!(Lang::from_path(&PathBuf::from("a/b.tsx")), Some(Lang::Tsx));
        assert_eq!(Lang::from_path(&PathBuf::from("a/b.ts")), Some(Lang::TypeScript));
        assert_eq!(Lang::from_path(&PathBuf::from("x.py")), Some(Lang::Python));
        assert_eq!(Lang::from_path(&PathBuf::from("x.txt")), None);
    }

    #[test]
    fn parsers_load() {
        for l in [Lang::Rust, Lang::TypeScript, Lang::Tsx, Lang::JavaScript, Lang::Python, Lang::Go, Lang::Java] {
            let lang = l.ts_language();
            assert!(lang.abi_version() > 0, "language {} has no abi_version", l.name());
            assert!(!l.tags_query().is_empty(), "tags query missing for {}", l.name());
        }
    }
}
