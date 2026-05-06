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
    Swift,
    C,
    Cpp,
    Ruby,
    Kotlin,
    CSharp,
    Php,
    Scala,
    Dart,
    Elixir,
    Lua,
    OCaml,
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
            Lang::Swift => "swift",
            Lang::C => "c",
            Lang::Cpp => "cpp",
            Lang::Ruby => "ruby",
            Lang::Kotlin => "kotlin",
            Lang::CSharp => "csharp",
            Lang::Php => "php",
            Lang::Scala => "scala",
            Lang::Dart => "dart",
            Lang::Elixir => "elixir",
            Lang::Lua => "lua",
            Lang::OCaml => "ocaml",
        }
    }

    pub fn from_path(path: &Path) -> Option<Lang> {
        let ext = path.extension()?.to_str()?;
        Lang::from_extension(ext)
    }

    pub fn resolution_group(self) -> &'static str {
        match self {
            Lang::TypeScript | Lang::Tsx | Lang::JavaScript => "js",
            Lang::C | Lang::Cpp => "c",
            Lang::Rust => "rust",
            Lang::Python => "py",
            Lang::Go => "go",
            Lang::Java => "java",
            Lang::Swift => "swift",
            Lang::Ruby => "ruby",
            Lang::Kotlin => "kotlin",
            Lang::CSharp => "csharp",
            Lang::Php => "php",
            Lang::Scala => "scala",
            Lang::Dart => "dart",
            Lang::Elixir => "elixir",
            Lang::Lua => "lua",
            Lang::OCaml => "ocaml",
        }
    }

    pub fn group_from_lang_name(name: &str) -> Option<&'static str> {
        match name {
            "typescript" | "tsx" | "javascript" => Some("js"),
            "c" | "cpp" => Some("c"),
            "rust" => Some("rust"),
            "python" => Some("py"),
            "go" => Some("go"),
            "java" => Some("java"),
            "swift" => Some("swift"),
            "ruby" => Some("ruby"),
            "kotlin" => Some("kotlin"),
            "csharp" => Some("csharp"),
            "php" => Some("php"),
            "scala" => Some("scala"),
            "dart" => Some("dart"),
            "elixir" => Some("elixir"),
            "lua" => Some("lua"),
            "ocaml" => Some("ocaml"),
            _ => None,
        }
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
            "swift" => Some(Lang::Swift),
            "c" | "h" => Some(Lang::C),
            "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => Some(Lang::Cpp),
            "rb" | "rake" => Some(Lang::Ruby),
            "kt" | "kts" => Some(Lang::Kotlin),
            "cs" => Some(Lang::CSharp),
            "php" => Some(Lang::Php),
            "scala" | "sc" => Some(Lang::Scala),
            "dart" => Some(Lang::Dart),
            "ex" | "exs" => Some(Lang::Elixir),
            "lua" => Some(Lang::Lua),
            "ml" | "mli" => Some(Lang::OCaml),
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
            Lang::Swift => tree_sitter_swift::LANGUAGE.into(),
            Lang::C => tree_sitter_c::LANGUAGE.into(),
            Lang::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Lang::Ruby => tree_sitter_ruby::LANGUAGE.into(),
            Lang::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
            Lang::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
            Lang::Php => tree_sitter_php::LANGUAGE_PHP.into(),
            Lang::Scala => tree_sitter_scala::LANGUAGE.into(),
            Lang::Dart => tree_sitter_dart::LANGUAGE.into(),
            Lang::Elixir => tree_sitter_elixir::LANGUAGE.into(),
            Lang::Lua => tree_sitter_lua::LANGUAGE.into(),
            Lang::OCaml => tree_sitter_ocaml::LANGUAGE_OCAML.into(),
        }
    }

    pub fn tags_query(self) -> &'static str {
        match self {
            Lang::Rust => include_str!("queries/rust-tags.scm"),
            Lang::TypeScript | Lang::Tsx => concat!(
                include_str!("queries/javascript-tags.scm"),
                "\n",
                include_str!("queries/typescript-tags.scm"),
            ),
            Lang::JavaScript => include_str!("queries/javascript-tags.scm"),
            Lang::Python => include_str!("queries/python-tags.scm"),
            Lang::Go => include_str!("queries/go-tags.scm"),
            Lang::Java => include_str!("queries/java-tags.scm"),
            Lang::Swift => include_str!("queries/swift-tags.scm"),
            Lang::C => include_str!("queries/c-tags.scm"),
            Lang::Cpp => include_str!("queries/cpp-tags.scm"),
            Lang::Ruby => include_str!("queries/ruby-tags.scm"),
            Lang::Kotlin => include_str!("queries/kotlin-tags.scm"),
            Lang::CSharp => include_str!("queries/csharp-tags.scm"),
            Lang::Php => include_str!("queries/php-tags.scm"),
            Lang::Scala => include_str!("queries/scala-tags.scm"),
            Lang::Dart => include_str!("queries/dart-tags.scm"),
            Lang::Elixir => include_str!("queries/elixir-tags.scm"),
            Lang::Lua => include_str!("queries/lua-tags.scm"),
            Lang::OCaml => include_str!("queries/ocaml-tags.scm"),
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
        for l in [
            Lang::Rust, Lang::TypeScript, Lang::Tsx, Lang::JavaScript, Lang::Python,
            Lang::Go, Lang::Java, Lang::Swift, Lang::C, Lang::Cpp, Lang::Ruby, Lang::Kotlin,
            Lang::CSharp, Lang::Php, Lang::Scala, Lang::Dart, Lang::Elixir, Lang::Lua, Lang::OCaml,
        ] {
            let lang = l.ts_language();
            assert!(lang.abi_version() > 0, "language {} has no abi_version", l.name());
            assert!(!l.tags_query().is_empty(), "tags query missing for {}", l.name());
        }
    }

    #[test]
    fn detects_new_extensions() {
        assert_eq!(Lang::from_path(&PathBuf::from("a.swift")), Some(Lang::Swift));
        assert_eq!(Lang::from_path(&PathBuf::from("a.c")), Some(Lang::C));
        assert_eq!(Lang::from_path(&PathBuf::from("a.h")), Some(Lang::C));
        assert_eq!(Lang::from_path(&PathBuf::from("a.cpp")), Some(Lang::Cpp));
        assert_eq!(Lang::from_path(&PathBuf::from("a.hpp")), Some(Lang::Cpp));
        assert_eq!(Lang::from_path(&PathBuf::from("a.rb")), Some(Lang::Ruby));
        assert_eq!(Lang::from_path(&PathBuf::from("a.kt")), Some(Lang::Kotlin));
        assert_eq!(Lang::from_path(&PathBuf::from("a.kts")), Some(Lang::Kotlin));
    }
}
