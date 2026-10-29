use super::*;

#[test]
fn startup_recovery_retries_only_typed_availability_failures() {
    let absent = HelperFailure::Exit {
        code: Some(codeterm_types::server_bootstrap::BOOTSTRAP_ATTACH_UNAVAILABLE_EXIT_CODE),
        request_forwarded: true,
        detail: None,
    };
    assert!(ClientStartupFailure::attach(&absent).retryable());
    assert!(ClientStartupFailure::attach(&HelperFailure::Deadline {
        request_forwarded: true,
    }).retryable());
    assert!(ClientStartupFailure::launch(&HelperFailure::Exit {
        code: Some(3),
        request_forwarded: true,
        detail: Some("existing owner".into()),
    }).retryable());
    for failure in [
        HelperFailure::PreDispatch(ExactServerShutdownRefusal::HelperArtifactInvalid),
        HelperFailure::Exit { code: Some(1), request_forwarded: true, detail: Some("unsafe identity".into()) },
        HelperFailure::Io { message: "unknown transport failure".into(), request_forwarded: true },
    ] {
        assert!(!ClientStartupFailure::attach(&failure).retryable());
        assert!(!ClientStartupFailure::launch(&failure).retryable());
    }
    assert!(!ClientStartupFailure::from("unclassified failure").retryable());
    assert!(ClientStartupFailure::native_connection(io::ErrorKind::NotFound.into()).retryable());
    assert!(!ClientStartupFailure::native_connection(io::ErrorKind::PermissionDenied.into()).retryable());
}

#[tokio::test]
async fn newline_observation_never_replaces_exact_eof_validation() {
    let (mut writer, stream) = tokio::io::duplex(64);
    let (observed, newline) = tokio::sync::oneshot::channel();
    let read = tokio::spawn(async move {
        let mut reader = tokio::io::BufReader::new(stream);
        read_single_bounded_line_observed(&mut reader, 32, || {
            let _ = observed.send(());
        })
        .await
    });
    writer.write_all(b"opaque\n").await.unwrap();
    newline.await.unwrap();
    assert!(!read.is_finished());
    writer.write_all(b"trailing").await.unwrap();
    writer.shutdown().await.unwrap();
    assert_eq!(
        read.await.unwrap().unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
}
