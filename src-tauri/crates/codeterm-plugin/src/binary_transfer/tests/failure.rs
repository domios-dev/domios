use super::*;
#[test]
fn failed_responses_remove_only_the_attempt_partial_and_never_publish() {
    let mut oversized=Reply::bytes(vec![1,2,3]);oversized.length=None;
    let mut spoofed=Reply::bytes(b"<svg>active</svg>".to_vec());spoofed.mime="image/png".into();
    let mut truncated=Reply::bytes(vec![0,255]);truncated.length=Some(3);
    let mut failed=Reply::bytes(vec![0,255]);failed.failure=Some(TransferRefusal::TransportUnconfirmed);
    let cases=[(oversized,TransferRefusal::TooLarge),(spoofed,TransferRefusal::MimeMismatch),(truncated,TransferRefusal::FileChanged),(failed,TransferRefusal::TransportUnconfirmed),(Reply::bytes(vec![0,255]),TransferRefusal::DigestMismatch)];
    for (reply,expected) in cases {
        let (_directory,session,_,_network)=fixture(vec![reply]);
        std::fs::write(session.root.join("sentinel"),b"preserve").unwrap();
        let mut request=request();
        if expected==TransferRefusal::TooLarge{if let Output::File{max_bytes,..}=&mut request.response{*max_bytes=2;}}
        if expected==TransferRefusal::MimeMismatch{if let Output::File{content_type,..}=&mut request.response{*content_type="image/png".into();}}
        if expected==TransferRefusal::DigestMismatch{if let Output::File{sha256,..}=&mut request.response{*sha256=Some("0".repeat(64));}}
        let error=execute(session.clone(),request,TransferAbort::default()).unwrap_err();assert_eq!(error.error,expected);
        assert_eq!(std::fs::read(session.root.join("sentinel")).unwrap(),b"preserve");
        assert_eq!(entries(&session.root),vec!["sentinel"]);
    }
}
struct Pending {started:std::sync::mpsc::Sender<()>}
#[async_trait]
impl TransferNetwork for Pending {
    async fn request(&self,_:&Url,_:&str,_:HeaderMap,_:Upload)->Result<NetworkResponse> {
        self.started.send(()).unwrap();futures_util::future::pending().await
    }
}
#[test]
fn abort_and_epoch_revocation_release_pending_transport_without_timers_or_retries() {
    for revoke in [false,true] {
        let (_directory,session,_,_)=fixture(vec![]);
        let (started,entered)=std::sync::mpsc::channel();
        *session.test_network.lock()=Some(Arc::new(Pending {started}));
        let abort=TransferAbort::default();let worker_abort=abort.clone();let worker_session=session.clone();
        let worker=std::thread::spawn(move ||execute(worker_session,request(),worker_abort));
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        if revoke {session.revoke();}else{abort.abort();}
        let error=worker.join().unwrap().unwrap_err();
        assert_eq!(error.error,if revoke{TransferRefusal::NotAllowed}else{TransferRefusal::Aborted});
        assert!(error.request_may_have_started);assert!(entries(&session.root).is_empty());
    }
}
struct Partial {entered:Mutex<Option<std::sync::mpsc::Sender<()>>>}
#[async_trait]
impl TransferNetwork for Partial {
    async fn request(&self,_:&Url,_:&str,_:HeaderMap,_:Upload)->Result<NetworkResponse> {
        let sender=self.entered.lock().take().unwrap();
        let stream=stream::iter([Ok(vec![0,255])]).chain(stream::once(async move {sender.send(()).unwrap();futures_util::future::pending::<Result<Vec<u8>>>().await})).boxed();
        Ok(NetworkResponse {status:200,content_type:Some("application/octet-stream".into()),location:None,length:None,stream,upload:None})
    }
}
#[test]
fn abort_after_partial_write_cleans_the_owned_file() {
    let (_directory,session,_,_)=fixture(vec![]);
    let (started,entered)=std::sync::mpsc::channel();*session.test_network.lock()=Some(Arc::new(Partial {entered:Mutex::new(Some(started))}));
    let abort=TransferAbort::default();let worker_abort=abort.clone();let worker_session=session.clone();
    let worker=std::thread::spawn(move ||execute(worker_session,request(),worker_abort));
    entered.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(entries(&session.root).len(),1);abort.abort();
    assert_eq!(worker.join().unwrap().unwrap_err().error,TransferRefusal::Aborted);
    assert!(entries(&session.root).is_empty());
}
#[test]
fn descriptor_and_multipart_bounds_refuse_without_a_request() {
    for descriptor in [serde_json::json!({"maxUploadBytes":MAX_BYTES+1}),serde_json::json!({"maxDownloadBytes":0}),serde_json::json!({"timeoutMs":MAX_TIMEOUT_MS+1})] {
        let descriptor:BinaryTransferDescriptor=serde_json::from_value(descriptor).unwrap();assert!(!descriptor.valid());
    }
    let (_directory,session,_,network)=fixture(vec![]);
    for parts in [vec![],(0..17).map(|_|Part::Text {name:"x".into(),value:"x".into()}).collect(),vec![Part::Text {name:"x".into(),value:"x".repeat(65537)}],vec![Part::Text {name:"bad
header".into(),value:"x".into()}]] {
        let mut request=request();request.method="POST".into();request.body=Some(InputBody::Multipart {parts});assert!(execute(session.clone(),request,TransferAbort::default()).is_err());
    }
    assert!(network.calls.lock().is_empty());
}
