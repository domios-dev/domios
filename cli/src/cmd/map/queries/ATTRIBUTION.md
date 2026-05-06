# Vendored tree-sitter `tags.scm` queries

These `*-tags.scm` files are derived from
[Aider](https://github.com/Aider-AI/aider) (Apache License 2.0).

| File | Source path in upstream |
|------|--------------------------|
| `rust-tags.scm`        | `aider/queries/tree-sitter-language-pack/rust-tags.scm`        |
| `javascript-tags.scm`  | `aider/queries/tree-sitter-language-pack/javascript-tags.scm`  |
| `python-tags.scm`      | `aider/queries/tree-sitter-language-pack/python-tags.scm`      |
| `go-tags.scm`          | `aider/queries/tree-sitter-language-pack/go-tags.scm`          |
| `java-tags.scm`        | `aider/queries/tree-sitter-language-pack/java-tags.scm`        |
| `typescript-tags.scm`  | `aider/queries/tree-sitter-languages/typescript-tags.scm`      |

Apache-2.0 license preserved at:
<https://github.com/Aider-AI/aider/blob/main/LICENSE.txt>

The captures (`name.definition.<kind>`, `name.reference.<kind>`) are consumed
by `cli/src/cmd/map/tags.rs`. Adding a new language = drop a new `.scm`,
extend `cli/src/cmd/map/lang.rs`, add the parser crate to `cli/Cargo.toml`.
