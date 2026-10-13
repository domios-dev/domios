use quote::ToTokens;
use std::{
    env, fs,
    path::{Path, PathBuf},
};
use syn::{
    visit::{self, Visit},
    Attribute, Item,
};

fn platform(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|a| {
        let text = a.to_token_stream().to_string();
        (a.path().is_ident("cfg") || a.path().is_ident("cfg_attr"))
            && text
                .split(|c: char| !c.is_alphanumeric() && c != '_')
                .any(|s| {
                    matches!(
                        s,
                        "windows"
                            | "unix"
                            | "target_os"
                            | "target_family"
                            | "target_arch"
                            | "target_env"
                            | "target_vendor"
                            | "target_pointer_width"
                    )
                })
    })
}

fn ignore_reason(attrs: &[Attribute]) -> Option<String> {
    fn reason(meta: &syn::Meta) -> Option<String> {
        match meta {
            syn::Meta::NameValue(value) if value.path.is_ident("ignore") => {
                if let syn::Expr::Lit(value) = &value.value {
                    if let syn::Lit::Str(value) = &value.lit {
                        let text = value.value();
                        return (!text.trim().is_empty()).then_some(text);
                    }
                }
                None
            }
            syn::Meta::List(list) if list.path.is_ident("cfg_attr") => {
                let nested = list
                    .parse_args_with(
                        syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
                    )
                    .ok()?;
                nested.iter().skip(1).find_map(reason)
            }
            _ => None,
        }
    }
    attrs.iter().find_map(|attr| reason(&attr.meta))
}

fn platform_tokens(text: &str) -> bool {
    let words: Vec<_> = text
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .collect();
    words.iter().any(|w| matches!(*w, "cfg" | "cfg_attr"))
        && words.iter().any(|w| {
            matches!(
                *w,
                "windows"
                    | "unix"
                    | "target_os"
                    | "target_family"
                    | "target_arch"
                    | "target_env"
                    | "target_vendor"
                    | "target_pointer_width"
            )
        })
}

#[derive(Default)]
struct BodyPlatform(bool);
impl<'ast> Visit<'ast> for BodyPlatform {
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if mac.path.is_ident("cfg") {
            self.0 |= platform_tokens(&format!("cfg {}", mac.tokens));
        }
        visit::visit_macro(self, mac);
    }

    fn visit_attribute(&mut self, attr: &'ast Attribute) {
        self.0 |= platform(std::slice::from_ref(attr));
        visit::visit_attribute(self, attr);
    }
}

fn include_path(tokens: impl ToTokens, source: &Path, out_dir: Option<&Path>) -> PathBuf {
    let tokens = tokens.to_token_stream();
    if let Ok(literal) = syn::parse2::<syn::LitStr>(tokens.clone()) {
        return source.join(literal.value());
    }
    let expression: syn::ExprMacro = syn::parse2(tokens).expect("unsupported include expression");
    assert!(
        expression.mac.path.is_ident("concat"),
        "unsupported include macro"
    );
    let parts = expression
        .mac
        .parse_body_with(syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated)
        .expect("include concat arguments");
    let mut value = String::new();
    for part in parts {
        match part {
            syn::Expr::Lit(lit) => match lit.lit {
                syn::Lit::Str(text) => value.push_str(&text.value()),
                _ => panic!("include concat requires strings"),
            },
            syn::Expr::Macro(mac) if mac.mac.path.is_ident("env") => {
                let name: syn::LitStr = mac.mac.parse_body().expect("include env name");
                assert_eq!(name.value(), "OUT_DIR", "unsupported include environment");
                value.push_str(
                    out_dir
                        .expect("include requires Cargo build-script OUT_DIR")
                        .to_str()
                        .expect("OUT_DIR UTF-8"),
                );
            }
            _ => panic!("unsupported include concat expression"),
        }
    }
    source.join(value)
}

fn walk(
    file: &Path,
    directory: &Path,
    prefix: &str,
    inherited: bool,
    output: &mut Vec<serde_json::Value>,
    out_dir: Option<&Path>,
) {
    let text = fs::read_to_string(file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
    let parsed = syn::parse_file(&text).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
    items(
        &parsed.items,
        directory,
        file.parent().unwrap(),
        prefix,
        inherited || platform(&parsed.attrs),
        output,
        out_dir,
    );
}

fn items(
    declarations: &[Item],
    directory: &Path,
    source_directory: &Path,
    prefix: &str,
    inherited: bool,
    output: &mut Vec<serde_json::Value>,
    out_dir: Option<&Path>,
) {
    for item in declarations {
        match item {
            Item::Mod(module) => {
                let name = module.ident.to_string();
                let next_prefix = format!("{prefix}{name}::");
                let gated = inherited || platform(&module.attrs);
                if let Some((_, body)) = &module.content {
                    items(
                        body,
                        &directory.join(&name),
                        &directory.join(&name),
                        &next_prefix,
                        gated,
                        output,
                        out_dir,
                    );
                } else {
                    let explicit = module.attrs.iter().find_map(|attr| {
                        if !attr.path().is_ident("path") {
                            return None;
                        }
                        if let syn::Meta::NameValue(value) = &attr.meta {
                            if let syn::Expr::Lit(value) = &value.value {
                                if let syn::Lit::Str(value) = &value.lit {
                                    return Some(value.value());
                                }
                            }
                        }
                        None
                    });
                    let flat = directory.join(format!("{name}.rs"));
                    let nested = directory.join(&name).join("mod.rs");
                    let path = explicit
                        .map(|p| source_directory.join(p))
                        .unwrap_or_else(|| if flat.exists() { flat } else { nested });
                    if !path.exists() {
                        panic!("unresolved module {}: {}", next_prefix, path.display());
                    }
                    let child_dir = if path.file_name().unwrap() == "mod.rs" {
                        path.parent().unwrap().to_path_buf()
                    } else {
                        path.parent().unwrap().join(path.file_stem().unwrap())
                    };
                    walk(&path, &child_dir, &next_prefix, gated, output, out_dir);
                }
            }
            Item::Fn(function) => {
                let is_test = function
                    .attrs
                    .iter()
                    .any(|a| a.path().segments.last().is_some_and(|s| s.ident == "test"));
                if is_test {
                    let mut body = BodyPlatform::default();
                    body.visit_block(&function.block);
                    output.push(serde_json::json!({
                        "name": format!("{prefix}{}", function.sig.ident),
                        "platform": inherited || platform(&function.attrs) || body.0,
                        "ignore_reason": ignore_reason(&function.attrs),
                    }));
                }
            }
            Item::Macro(invocation) if invocation.mac.path.is_ident("include") => {
                let path = include_path(invocation.mac.tokens.clone(), source_directory, out_dir);
                walk(
                    &path,
                    directory,
                    prefix,
                    inherited || platform(&invocation.attrs),
                    output,
                    out_dir,
                );
            }
            Item::Macro(invocation) if invocation.ident.is_none() => {
                let definition = declarations.iter().find_map(|item| match item {
                    Item::Macro(definition)
                        if definition
                            .ident
                            .as_ref()
                            .is_some_and(|name| invocation.mac.path.is_ident(name)) =>
                    {
                        Some(definition)
                    }
                    _ => None,
                });
                if let Some(definition) = definition {
                    output.push(serde_json::json!({
                        "name": prefix,
                        "scope": true,
                        "platform": inherited || platform(&invocation.attrs) || platform_tokens(&definition.mac.tokens.to_string()),
                    }));
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let root = PathBuf::from(
        env::args()
            .nth(1)
            .expect("usage: ci-test-classifier ROOT_RS"),
    );
    let mut output = Vec::new();
    let out_dir = env::args().nth(2).map(PathBuf::from);
    walk(
        &root,
        root.parent().unwrap(),
        "",
        false,
        &mut output,
        out_dir.as_deref(),
    );
    println!("{}", serde_json::to_string(&output).unwrap());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(source: &str) -> Vec<serde_json::Value> {
        let file = syn::parse_file(source).unwrap();
        let mut output = Vec::new();
        items(
            &file.items,
            Path::new("."),
            Path::new("."),
            "",
            false,
            &mut output,
            None,
        );
        output
    }

    #[test]
    fn resolves_generated_include_from_exact_cargo_output() {
        let tokens = quote::quote!(concat!(env!("OUT_DIR"), "/generated.rs"));
        assert_eq!(
            include_path(tokens, Path::new("src"), Some(Path::new("/build/out"))),
            PathBuf::from("/build/out/generated.rs")
        );
        assert_eq!(
            include_path(quote::quote!("tests.rs"), Path::new("src"), None),
            PathBuf::from("src/tests.rs")
        );
    }

    #[test]
    #[should_panic(expected = "Cargo build-script OUT_DIR")]
    fn missing_generated_directory_refuses_classification() {
        include_path(
            quote::quote!(concat!(env!("OUT_DIR"), "/generated.rs")),
            Path::new("src"),
            None,
        );
    }

    #[test]
    fn inherits_inline_module_cfg() {
        let rows =
            classify("#[cfg(unix)] mod tests { #[test] fn native() {} } #[test] fn shared() {}");
        assert_eq!(rows[0]["name"], "tests::native");
        assert_eq!(rows[0]["platform"], true);
        assert_eq!(rows[1]["platform"], false);
    }

    #[test]
    fn detects_platform_cfg_inside_function() {
        let rows = classify("#[tokio::test] async fn native() { #[cfg(windows)] { run(); } }");
        assert_eq!(rows[0]["platform"], true);
    }

    #[test]
    fn fixture_gate_removal_moves_test_to_shared() {
        let before = classify("#[cfg_attr(not(windows), ignore)] #[test] fn fixture() {}");
        let after = classify("#[test] fn fixture() {}");
        assert_eq!(before[0]["platform"], true);
        assert_eq!(after[0]["platform"], false);
    }

    #[test]
    fn classifies_cfg_macro_and_generated_test_scopes() {
        let rows = classify("#[test] fn native() { if cfg!(windows) {} } macro_rules! cases { () => { #[test] fn generated() {} }; } cases!();");
        assert_eq!(rows[0]["platform"], true);
        assert_eq!(rows[1]["scope"], true);
        assert_eq!(rows[1]["platform"], false);
    }

    #[test]
    fn preserves_explicit_ignore_reasons_only() {
        let rows = classify("#[ignore = \"TOKEN_REASON\"] #[test] fn named() {} #[ignore] #[test] fn anonymous() {}");
        assert_eq!(rows[0]["ignore_reason"], "TOKEN_REASON");
        assert_eq!(rows[1]["ignore_reason"], serde_json::Value::Null);
    }

    #[test]
    fn feature_cfg_does_not_bind_test_to_os() {
        let rows =
            classify("#[cfg(feature = \"embeddings\")] mod tests { #[test] fn shared() {} }");
        assert_eq!(rows[0]["platform"], false);
    }
}
