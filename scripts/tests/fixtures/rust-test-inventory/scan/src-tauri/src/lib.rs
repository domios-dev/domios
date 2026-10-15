//! Doc braces { { {
mod outer;
mod sentinel;
#[path = "custom/located.rs"]
mod relocated;
#[cfg(target_os = "macos")]
mod mac_only;
mod generated;

pub fn braces<'a>(x: &'a str) -> &'a str {
    let _open = '{';
    let _close = '}';
    let _byte = b'}';
    let _escaped = '\'';
    let _s = "}}} { \" }";
    let _r = r#"} " { "#;
    let _b = br##"}"# {"##;
    /* block { /* nested } */ { */
    // line }
    x
}

#[cfg(test)]
mod tests {
    #[test]
    fn root_inline() {}

    mod nested {
        pub(crate) mod deeper {
            #[test]
            fn deep_test() {
                let _s = "}";
            }
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn tokio_flavored() {}

    #[tokio::test]
    pub async fn tokio_plain() {}

    #[test]
    fn r#match() {}

    fn helper() {
        #[test]
        fn inner_item() {}
    }

    include!("included_tests.rs");
}
