use super::*;
#[test]
fn absolute_and_parent_upload_paths_refuse_even_if_the_generic_read_authority_would_allow_them() {
    let (directory,session,_,network)=fixture(vec![]);std::fs::write(directory.path().join("outside"),[0,255]).unwrap();
    for path in ["../outside".to_owned(),directory.path().join("outside").to_string_lossy().into_owned(),"file:stream".into(),"C:/outside".into()] {
        let mut request=request();request.method="POST".into();request.body=Some(InputBody::File {file:Input::Owned {identity:None,path,content_type:"application/octet-stream".into(),file_name:"File.bin".into()}});
        assert_eq!(execute(session.clone(),request,TransferAbort::default()).unwrap_err().error,TransferRefusal::NotAllowed);
    }
    assert!(network.calls.lock().is_empty());
}
#[test]
fn selected_inputs_are_one_use_and_bound_to_plugin_epoch_and_origin() {
    let (directory,session,_,network)=fixture(vec![Reply::bytes(vec![0,255])]);
    let source=directory.path().join("selected");std::fs::write(&source,data()).unwrap();
    let handle=session.select_input("https://api.example",std::fs::File::open(&source).unwrap(),"Reference.bin","application/octet-stream",None).unwrap().handle;
    let mut selected=request();selected.method="PUT".into();selected.body=Some(InputBody::File {file:Input::Selected {handle:handle.clone()}});
    let mut foreign=selected.clone();foreign.url="https://cdn.example/output".into();
    assert_eq!(execute(session.clone(),foreign,TransferAbort::default()).unwrap_err().error,TransferRefusal::NotAllowed);
    let (_other_directory,other,_,_)=fixture(vec![]);
    assert_eq!(execute(other,selected.clone(),TransferAbort::default()).unwrap_err().error,TransferRefusal::NotAllowed);
    execute(session.clone(),selected.clone(),TransferAbort::default()).unwrap();
    assert_eq!(network.calls.lock()[0].2,data());
    assert_eq!(execute(session,selected,TransferAbort::default()).unwrap_err().error,TransferRefusal::NotAllowed);
    assert_eq!(network.calls.lock().len(),1);
}
#[cfg(unix)]
#[test]
fn uploads_refuse_symlinks_and_snapshot_growth_but_report_shrinkage() {
    use std::os::unix::fs::symlink;
    use std::io::Write;
    let (directory,session,_,network)=fixture(vec![]);
    std::fs::write(directory.path().join("outside"),data()).unwrap();symlink(directory.path().join("outside"),session.root.join("escape")).unwrap();
    let make=|path:&str|InputBody::File {file:Input::Owned {identity:None,path:path.into(),content_type:"application/octet-stream".into(),file_name:"File.bin".into()}};
    assert!(build(session.clone(),&Url::parse("https://api.example/").unwrap(),Some(make("escape")),TransferAbort::default(),std::time::Instant::now()+Duration::from_secs(2)).is_err());
    let file=session.root.join("source");std::fs::write(&file,[0,255]).unwrap();
    let body=build(session.clone(),&Url::parse("https://api.example/").unwrap(),Some(make("source")),TransferAbort::default(),std::time::Instant::now()+Duration::from_secs(2)).unwrap();
    std::fs::OpenOptions::new().append(true).open(&file).unwrap().write_all(&[1,2,3]).unwrap();
    let bytes=futures_executor::block_on(body.stream.try_collect::<Vec<_>>()).unwrap().concat();assert_eq!(bytes,[0,255]);
    let body=build(session.clone(),&Url::parse("https://api.example/").unwrap(),Some(make("source")),TransferAbort::default(),std::time::Instant::now()+Duration::from_secs(2)).unwrap();
    std::fs::OpenOptions::new().write(true).open(&file).unwrap().set_len(1).unwrap();
    assert_eq!(futures_executor::block_on(body.stream.try_collect::<Vec<_>>()).unwrap_err(),TransferRefusal::FileChanged);
    assert!(network.calls.lock().is_empty());
}
#[test]
fn replacing_one_runtime_revokes_only_that_runtime() {
    let (_directory,old,_,_)=fixture(vec![]);activate(&old);
    let (_directory2,new,_,_)=fixture(vec![Reply::bytes(vec![0,255])]);
    // A replacement carries the same host plugin identity and a fresh epoch.
    let replacement=TransferSession::at_root(old.plugin_id(),new.root.clone(),new.limits.clone(),new.authority.clone()).unwrap();
    *replacement.test_network.lock()=new.test_network.lock().clone();activate(&replacement);
    assert!(old.check().is_err());assert!(replacement.check().is_ok());
    execute(replacement.clone(),request(),TransferAbort::default()).unwrap();
    old.revoke();assert!(replacement.check().is_ok());deactivate(replacement.plugin_id());assert!(replacement.check().is_err());
}

#[test]
fn recorded_identity_refuses_replaced_bytes_before_dispatch_and_in_place_changes_while_streaming() {
    use sha2::{Digest,Sha256};
    let (_directory,session,_,network)=fixture(vec![]);
    let original=data();let path=session.root.join("input");std::fs::write(&path,&original).unwrap();
    let identity=FileIdentity{size:original.len()as u64,sha256:format!("{:x}",Sha256::digest(&original))};
    let input=Input::Owned{identity:Some(identity),path:"input".into(),content_type:"application/octet-stream".into(),file_name:"input.bin".into()};
    let mut replaced=original.clone();replaced[0]^=1;std::fs::write(&path,&replaced).unwrap();
    let mut request=request();request.method="POST".into();request.body=Some(InputBody::File{file:input.clone()});
    let error=execute(session.clone(),request,TransferAbort::default()).unwrap_err();assert_eq!(error.error,TransferRefusal::DigestMismatch);assert!(!error.request_may_have_started);assert!(network.calls.lock().is_empty());
    #[cfg(unix)]
    {
    std::fs::write(&path,&original).unwrap();
    let mut body=build(session.clone(),&Url::parse("https://api.example/").unwrap(),Some(InputBody::File{file:input}),TransferAbort::default(),std::time::Instant::now()+Duration::from_secs(2)).unwrap();
    assert_eq!(futures_executor::block_on(body.stream.next()).unwrap().unwrap(),original[..CHUNK]);
    let mut changed=original;changed[CHUNK]^=1;std::fs::write(&path,&changed).unwrap();
    assert_eq!(futures_executor::block_on(body.stream.try_collect::<Vec<_>>()).unwrap_err(),TransferRefusal::DigestMismatch);
    assert_eq!(body.state.finish(),Err(TransferRefusal::DigestMismatch));
    }
}

#[test]
fn prepared_grants_revoke_on_failed_postvalidation_and_never_cross_epoch_replacement(){
    let (directory,session,_,_)=fixture(vec![]);activate(&session);
    let path=directory.path().join("selected");std::fs::write(&path,[0,255]).unwrap();
    let pending=prepare_plugin_input(session.plugin_id(),"https://api.example",std::fs::File::open(&path).unwrap(),"input.bin","application/octet-stream",None).unwrap();
    let handle=pending.handle_for_test().to_owned();drop(pending);
    assert!(session.take_input(&handle,&Url::parse("https://api.example/").unwrap()).is_err());
    let pending=prepare_plugin_input(session.plugin_id(),"https://api.example",std::fs::File::open(&path).unwrap(),"input.bin","application/octet-stream",None).unwrap();
    let replacement=TransferSession::at_root(session.plugin_id(),session.root.clone(),session.limits.clone(),session.authority.clone()).unwrap();activate(&replacement);
    assert_eq!(pending.commit().unwrap_err(),TransferRefusal::NotAllowed);assert!(replacement.check().is_ok());deactivate(replacement.plugin_id());
}
