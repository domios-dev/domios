use super::*;

#[test]
fn browser_code_is_written_once_with_one_line_terminator() {
    crate::async_runtime::block_on(async {
        let (mut input, mut output) = tokio::io::duplex(64);
        let mut bytes = [0u8; 7];
        let (submitted, read) = tokio::join!(
            submit_code(&mut input, "opaque"),
            output.read_exact(&mut bytes)
        );
        submitted.unwrap();
        read.unwrap();
        assert_eq!(&bytes, b"opaque\n");
    });
}

#[test]
fn captures_complete_provider_link_from_osc_hyperlink_only() {
    crate::async_runtime::block_on(async {
        let pattern =
            regex::Regex::new(r"(https://example\.test/oauth/authorize\?[^\s\x1b]+)[\s\x1b]")
                .unwrap();
        let mut found = Vec::new();
        let output = b"open this: \x1b]8;;https://example.test/oauth/authorize?client_id=fixture&state=opaque\x1b\\Sign in\x1b]8;;\x1b\\";
        capture(&output[..], |url| found.push(url), &pattern)
            .await
            .unwrap();
        assert_eq!(
            found,
            ["https://example.test/oauth/authorize?client_id=fixture&state=opaque"]
        );
        let mut foreign = Vec::new();
        capture(
            &b"https://foreign.test/foreign\n"[..],
            |url| foreign.push(url),
            &pattern,
        )
        .await
        .unwrap();
        assert!(foreign.is_empty());
    });
}

#[test]
fn bounded_sign_in_capture_rejects_unbounded_output() {
    crate::async_runtime::block_on(async {
        let pattern = regex::Regex::new(r"(https://example\.test/[^\s]+)\s").unwrap();
        let bytes = vec![b'x'; 65_537];
        assert!(capture(&bytes[..], |_| {}, &pattern).await.is_err());
    });
}
