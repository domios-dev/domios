# Vendored tree-sitter `tags.scm` queries

The `*-tags.scm` files in this directory are copied (or, for Kotlin,
written from scratch following the same conventions) from the upstream
tree-sitter grammar projects below. All of them are MIT-licensed except
where noted.

| File | Upstream source | License |
|------|-----------------|---------|
| `rust-tags.scm`        | [tree-sitter/tree-sitter-rust](https://github.com/tree-sitter/tree-sitter-rust) `queries/tags.scm`           | MIT |
| `typescript-tags.scm`  | [tree-sitter/tree-sitter-typescript](https://github.com/tree-sitter/tree-sitter-typescript) `queries/tags.scm` | MIT |
| `javascript-tags.scm`  | [tree-sitter/tree-sitter-javascript](https://github.com/tree-sitter/tree-sitter-javascript) `queries/tags.scm` | MIT |
| `python-tags.scm`      | [tree-sitter/tree-sitter-python](https://github.com/tree-sitter/tree-sitter-python) `queries/tags.scm`         | MIT |
| `go-tags.scm`          | [tree-sitter/tree-sitter-go](https://github.com/tree-sitter/tree-sitter-go) `queries/tags.scm`                 | MIT |
| `java-tags.scm`        | [tree-sitter/tree-sitter-java](https://github.com/tree-sitter/tree-sitter-java) `queries/tags.scm`             | MIT |
| `swift-tags.scm`       | [alex-pinkus/tree-sitter-swift](https://github.com/alex-pinkus/tree-sitter-swift) `queries/tags.scm`           | MIT |
| `c-tags.scm`           | [tree-sitter/tree-sitter-c](https://github.com/tree-sitter/tree-sitter-c) `queries/tags.scm`                   | MIT |
| `cpp-tags.scm`         | [tree-sitter/tree-sitter-cpp](https://github.com/tree-sitter/tree-sitter-cpp) `queries/tags.scm`               | MIT |
| `ruby-tags.scm`        | [tree-sitter/tree-sitter-ruby](https://github.com/tree-sitter/tree-sitter-ruby) `queries/tags.scm`             | MIT |
| `csharp-tags.scm`      | [tree-sitter/tree-sitter-c-sharp](https://github.com/tree-sitter/tree-sitter-c-sharp) `queries/tags.scm`       | MIT |
| `php-tags.scm`         | [tree-sitter/tree-sitter-php](https://github.com/tree-sitter/tree-sitter-php) `queries/tags.scm`               | MIT |
| `scala-tags.scm`       | [tree-sitter/tree-sitter-scala](https://github.com/tree-sitter/tree-sitter-scala) `queries/tags.scm`           | MIT |
| `dart-tags.scm`        | [UserNobody14/tree-sitter-dart](https://github.com/UserNobody14/tree-sitter-dart) `queries/tags.scm`           | MIT |
| `elixir-tags.scm`      | [elixir-lang/tree-sitter-elixir](https://github.com/elixir-lang/tree-sitter-elixir) `queries/tags.scm`         | Apache-2.0 |
| `lua-tags.scm`         | [tree-sitter-grammars/tree-sitter-lua](https://github.com/tree-sitter-grammars/tree-sitter-lua) `queries/tags.scm` | MIT |
| `ocaml-tags.scm`       | [tree-sitter/tree-sitter-ocaml](https://github.com/tree-sitter/tree-sitter-ocaml) `queries/tags.scm`           | MIT |
| `kotlin-tags.scm`      | written from scratch, follows the same `@name` + `@definition.<kind>` convention | n/a |

The captures (`@name` + `@definition.<kind>` / `@reference.<kind>`) are
consumed by `cli/src/cmd/map/tags.rs`. Adding a new language: drop the
upstream `tags.scm`, extend `cli/src/cmd/map/lang.rs`, add the parser
crate to `cli/Cargo.toml`.
