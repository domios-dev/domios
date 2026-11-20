use super::*;
use crate::update::failure_report::{UpdateAbort, UpdateFailureSink};
use crate::update::orchestrate::install_after_shutdown;

struct FailureSink;

impl UpdateFailureSink for FailureSink {
    fn abort(&self, _: &UpdateAbort) {}
}

fn identity(version: &str, signature: &str) -> PayloadIdentity {
    PayloadIdentity {
        version: version.into(),
        signature: signature.into(),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn shutdown_refusal_keeps_verified_bytes_for_a_matching_retry() {
    let staged = StagedPayload::new();
    let bytes = staged
        .get_or_download(identity("2.0.0", "signed-digest"), async {
            Ok(vec![1, 2, 3])
        })
        .await
        .unwrap();
    let first = install_after_shutdown(
        bytes.clone(),
        || async { Ok(()) },
        |()| async { Err("shutdown refused".into()) },
        |_| async { panic!("shutdown refusal must prevent install") },
        &FailureSink,
    )
    .await;
    assert_eq!(first.unwrap_err(), "shutdown refused");

    let retry = staged
        .get_or_download(identity("2.0.0", "signed-digest"), async {
            panic!("a matching retry must not fetch the artifact");
        })
        .await
        .unwrap();
    assert!(Arc::ptr_eq(&bytes, &retry));
    install_after_shutdown(
        retry,
        || async { Ok(()) },
        |()| async { Ok(vec![]) },
        |bytes| async move {
            assert_eq!(bytes.as_slice(), [1, 2, 3]);
            Ok(())
        },
        &FailureSink,
    )
    .await
    .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_changed_digest_or_version_requires_a_new_verified_download() {
    let staged = StagedPayload::new();
    staged
        .get_or_download(identity("2.0.0", "first"), async { Ok(vec![1]) })
        .await
        .unwrap();

    let refused = staged
        .get_or_download(identity("2.0.0", "second"), async {
            Err("bad signature".into())
        })
        .await;
    assert_eq!(refused.unwrap_err(), "bad signature");
    let changed = staged
        .get_or_download(identity("2.0.0", "second"), async { Ok(vec![2]) })
        .await
        .unwrap();
    assert_eq!(changed.as_slice(), [2]);

    let newer = staged
        .get_or_download(identity("2.0.1", "second"), async { Ok(vec![3]) })
        .await
        .unwrap();
    assert_eq!(newer.as_slice(), [3]);
}
