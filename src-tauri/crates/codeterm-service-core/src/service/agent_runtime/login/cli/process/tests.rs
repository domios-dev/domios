use super::*;

#[test]
fn browser_code_is_written_once_with_one_line_terminator() {
    crate::async_runtime::block_on(async {
        let (mut input, mut output) = tokio::io::duplex(64);
        let mut bytes = [0u8; 7];
        let code = Zeroizing::new("opaque".to_owned());
        let (submitted, read) = tokio::join!(
            submit_code(&mut input, &code),
            output.read_exact(&mut bytes)
        );
        assert_eq!(submitted, CodeWriteOutcome::Written);
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

#[test]
fn submit_code_classifies_closed_pipe_before_write_as_prewrite_failure() {
    crate::async_runtime::block_on(async {
        let (mut input, output) = tokio::io::duplex(8);
        drop(output);
        let code = Zeroizing::new("opaque-code".to_owned());
        assert!(matches!(
            submit_code(&mut input, &code).await,
            CodeWriteOutcome::FailedBeforeWrite(_)
        ));
    });
}

#[test]
fn submit_code_classifies_mid_write_break_as_uncertain() {
    crate::async_runtime::block_on(async {
        let (mut input, mut output) = tokio::io::duplex(4);
        let code = Zeroizing::new("opaque-long-code".to_owned());
        let reader = tokio::spawn(async move {
            let mut partial = [0u8; 4];
            let _ = output.read_exact(&mut partial).await;
            drop(output);
        });
        let outcome = submit_code(&mut input, &code).await;
        reader.await.unwrap();
        assert!(
            matches!(outcome, CodeWriteOutcome::Uncertain(_)),
            "partial write followed by broken pipe must be uncertain, got {outcome:?}"
        );
    });
}
