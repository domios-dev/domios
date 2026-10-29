use super::*;
#[test]
fn json_and_empty_upload_receipts_preserve_status_without_creating_an_output_file() {
    use sha2::{Digest, Sha256};
    let bytes = br#"{"job":"opaque-job","state":"queued"}"#.to_vec();
    let mut json = Reply::bytes(bytes.clone());
    json.status = 202;
    json.mime = "application/json".into();
    let mut empty = Reply::bytes(vec![]);
    empty.status = 204;
    empty.mime = String::new();
    let (_directory, session, _, network) = fixture(vec![json, empty]);
    std::fs::write(session.root.join("input"), [0, 255]).unwrap();
    let mut json = request();
    json.method = "POST".into();
    json.body = Some(InputBody::Multipart {
        parts: vec![Part::File {
            name: "reference".into(),
            file: Input::Owned {
                identity: None,
                path: "input".into(),
                content_type: "application/octet-stream".into(),
                file_name: "reference.bin".into(),
            },
        }],
    });
    json.response = Output::Json {
        max_bytes: 1024,
        statuses: vec![202],
    };
    match execute(session.clone(), json, TransferAbort::default()).unwrap() {
        TransferReceipt::Json {
            status,
            data,
            size,
            sha256,
        } => {
            assert_eq!(status, 202);
            assert_eq!(data["job"], "opaque-job");
            assert_eq!(size, bytes.len() as u64);
            assert_eq!(sha256, format!("{:x}", Sha256::digest(&bytes)));
        }
        other => panic!("unexpected receipt: {other:?}"),
    }
    let mut empty = request();
    empty.method = "PUT".into();
    empty.response = Output::Empty {
        statuses: vec![204],
    };
    assert!(matches!(
        execute(session.clone(), empty, TransferAbort::default()).unwrap(),
        TransferReceipt::Empty { status: 204 }
    ));
    assert_eq!(network.calls.lock().len(), 2);
    assert_eq!(entries(&session.root), vec!["input"]);
}
#[test]
fn invalid_metadata_after_dispatch_preserves_uncertainty_status_and_no_partial_files() {
    let cases = [
        (
            b"not json".to_vec(),
            "application/json",
            64,
            200,
            TransferRefusal::InvalidResponse,
        ),
        (
            vec![255],
            "application/json",
            64,
            200,
            TransferRefusal::InvalidResponse,
        ),
        (
            b"{}".to_vec(),
            "text/html",
            64,
            200,
            TransferRefusal::MimeMismatch,
        ),
        (
            b"{}".to_vec(),
            "application/json",
            1,
            200,
            TransferRefusal::TooLarge,
        ),
        (
            b"{}".to_vec(),
            "application/json",
            64,
            503,
            TransferRefusal::HttpStatus,
        ),
    ];
    for (bytes, mime, limit, status, reason) in cases {
        let mut reply = Reply::bytes(bytes);
        reply.mime = mime.into();
        reply.status = status;
        let (_directory, session, _, network) = fixture(vec![reply]);
        let mut request = request();
        request.method = "POST".into();
        request.response = Output::Json {
            max_bytes: limit,
            statuses: vec![200],
        };
        let error = execute(session.clone(), request, TransferAbort::default()).unwrap_err();
        assert_eq!(error.error, reason);
        assert!(error.request_may_have_started);
        assert_eq!(error.status, Some(status));
        assert_eq!(network.calls.lock().len(), 1);
        assert!(entries(&session.root).is_empty());
    }
    let mut reply = Reply::bytes(vec![1]);
    reply.status = 204;
    let (_directory, session, _, _) = fixture(vec![reply]);
    let mut request = request();
    request.response = Output::Empty {
        statuses: vec![204],
    };
    let error = execute(session, request, TransferAbort::default()).unwrap_err();
    assert_eq!(error.error, TransferRefusal::InvalidResponse);
    assert!(error.request_may_have_started);
    assert_eq!(error.status, Some(204));
}
#[test]
fn invalid_response_contracts_refuse_before_dispatch() {
    let (_directory, session, _, network) = fixture(vec![]);
    for output in [
        Output::Json {
            max_bytes: MAX_METADATA_BYTES + 1,
            statuses: vec![200],
        },
        Output::Empty { statuses: vec![] },
        Output::Empty {
            statuses: vec![302],
        },
        Output::Empty {
            statuses: vec![200, 200],
        },
    ] {
        let mut request = request();
        request.response = output;
        let error = execute(session.clone(), request, TransferAbort::default()).unwrap_err();
        assert_eq!(error.error, TransferRefusal::InvalidRequest);
        assert!(!error.request_may_have_started);
    }
    assert!(network.calls.lock().is_empty());
}
