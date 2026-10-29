use super::*;
struct Lease {plugin:String,frame:String}
impl Drop for Lease{fn drop(&mut self){revoke_frame(&self.plugin,&self.frame);}}
fn install()->(tempfile::TempDir,Arc<dyn ViewProvider>,ViewRegistryHandle,Lease){
    let (directory,_store,provider,_frame,_now,registry)=crate::media_delivery::tests::fixture();
    let registration=create_frame(&provider,&registry,crate::view_host::ViewCallContext::panel(provider.id())).unwrap();
    let lease=Lease{plugin:provider.id().into(),frame:registration.media_frame};(directory,provider,registry,lease)
}
fn png()->Vec<u8>{b"\x89PNG\r\n\x1a\nopaque-pixels".to_vec()}
fn limits(accept:&[String])->IngestLimits<'_>{IngestLimits{accept,max_bytes:128}}
#[test]
fn byte_reader_and_opened_inputs_copy_exact_bytes_into_the_same_frame_store(){
    for kind in 0..3{
        let (directory,provider,_registry,lease)=install();let bytes=png();let accept=vec!["image/png".into()];
        let external=directory.path().join("selected.png");std::fs::write(&external,&bytes).unwrap();
        let source=match kind{0=>IngestSource::Bytes(bytes.clone()),1=>IngestSource::Reader(Box::new(std::io::Cursor::new(bytes.clone()))),_=>IngestSource::Opened(File::open(&external).unwrap())};
        let input=ingest_at_root(provider.id(),&lease.frame,directory.path(),source,"../CON.exe","image/png",limits(&accept)).unwrap();
        let mut opened=open_frame_media(provider.id(),&lease.frame,&input.handle).unwrap();let mut saved=Vec::new();opened.file.read_to_end(&mut saved).unwrap();
        assert_eq!(saved,bytes);assert_eq!(opened.size,bytes.len()as u64);assert_eq!(input.sha256,format!("{:x}",Sha256::digest(&bytes)));assert_eq!(input.content_type,"image/png");assert!(!input.file_name.contains('/'));assert!(!input.file_name.ends_with(".exe"));
        assert!(opened.path.starts_with(directory.path().canonicalize().unwrap().join("inputs")));assert!(open_frame_media("foreign",&lease.frame,&input.handle).is_err());
        revoke_frame(provider.id(),&lease.frame);assert!(open_frame_media(provider.id(),&lease.frame,&input.handle).is_err());
    }
}
#[test]
fn refused_inputs_leave_no_partial_file_or_live_handle(){
    for (bytes,hint,max_bytes,reason) in [(png(),"image/jpeg",128,IngestRefusal::TypeNotAccepted),(b"<svg>active</svg>".to_vec(),"image/png",128,IngestRefusal::TypeNotAccepted),(vec![0;129],"image/png",128,IngestRefusal::TooLarge),(png(),"image/png",1,IngestRefusal::TooLarge)]{
        let (directory,provider,_registry,lease)=install();let accept=vec!["image/png".into()];
        let error=ingest_at_root(provider.id(),&lease.frame,directory.path(),IngestSource::Reader(Box::new(std::io::Cursor::new(bytes))),"source.png",hint,IngestLimits{accept:&accept,max_bytes}).unwrap_err();assert_eq!(error,reason);
        assert_eq!(std::fs::read_dir(directory.path().join("inputs")).unwrap().count(),0);assert!(STORE.lock().frames[&lease.frame].handles.is_empty());
    }
}
struct RevokingReader {plugin:String,frame:String,first:bool}
impl Read for RevokingReader{fn read(&mut self,buffer:&mut[u8])->std::io::Result<usize>{if self.first{self.first=false;let bytes=png();buffer[..bytes.len()].copy_from_slice(&bytes);revoke_frame(&self.plugin,&self.frame);Ok(bytes.len())}else{Ok(0)}}}
#[test]
fn frame_revocation_during_ingestion_refuses_and_removes_partial(){
    let (directory,provider,_registry,lease)=install();let accept=vec!["image/png".into()];
    let reader=RevokingReader{plugin:provider.id().into(),frame:lease.frame.clone(),first:true};
    let error=ingest_at_root(provider.id(),&lease.frame,directory.path(),IngestSource::Reader(Box::new(reader)),"image.png","image/png",limits(&accept)).unwrap_err();assert_eq!(error,IngestRefusal::UnknownFrame);assert_eq!(std::fs::read_dir(directory.path().join("inputs")).unwrap().count(),0);
}

#[test]
fn transfer_input_requires_the_exact_registered_context_and_an_ingested_source(){
    let (directory,provider,registry,lease)=install();let accept=vec!["image/png".into()];
    let input=ingest_at_root(provider.id(),&lease.frame,directory.path(),IngestSource::Bytes(png()),"reference.png","image/png",limits(&accept)).unwrap();
    let context=frame_context(provider.id(),&lease.frame).unwrap();
    assert!(context.frame.is_some());assert_eq!(open_context_input(provider.id(),&context,&input.handle).unwrap().sha256,input.sha256);
    let mut forged=context.clone();forged.frame=Some("foreign".into());assert!(open_context_input(provider.id(),&forged,&input.handle).is_err());
    let mut forged=context.clone();forged.owner=Some("foreign".into());assert!(open_context_input(provider.id(),&forged,&input.handle).is_err());
    let output=register_at_root(&mut STORE.lock(),provider.id(),&lease.frame,directory.path(),MediaFile{path:"clip.mp4".into(),content_type:MediaType::Mp4,file_name:None},Instant::now()).unwrap();
    assert!(open_context_input(provider.id(),&context,&output.handle).is_err());
    registry.unregister(provider.id());assert!(open_context_input(provider.id(),&context,&input.handle).is_err());
}

struct RemintingReader {provider:Arc<dyn ViewProvider>,registry:ViewRegistryHandle,frame:String,fresh:Arc<Mutex<Option<FrameRegistration>>>,first:bool}
impl Read for RemintingReader{
    fn read(&mut self,buffer:&mut[u8])->std::io::Result<usize>{
        if !self.first{return Ok(0);}self.first=false;revoke_frame(self.provider.id(),&self.frame);
        let registered=reregister_frame(&self.provider,&self.registry,crate::view_host::ViewCallContext::panel(self.provider.id()),&self.frame).unwrap();*self.fresh.lock()=Some(registered);
        let bytes=png();buffer[..bytes.len()].copy_from_slice(&bytes);Ok(bytes.len())
    }
}
#[test]
fn in_flight_ingestion_cannot_move_into_a_reminted_frame_or_retire_the_new_document(){
    let (directory,provider,registry,lease)=install();let accept=vec!["image/png".into()];let old=frame_context(provider.id(),&lease.frame).unwrap();
    let fresh=Arc::new(Mutex::new(None));let source=RemintingReader{provider:provider.clone(),registry,frame:lease.frame.clone(),fresh:fresh.clone(),first:true};
    let error=ingest_at_root(provider.id(),&lease.frame,directory.path(),IngestSource::Reader(Box::new(source)),"image.png","image/png",limits(&accept)).unwrap_err();assert_eq!(error,IngestRefusal::UnknownFrame);
    let context=fresh.lock().as_ref().unwrap().context.clone();assert_ne!(context,old);context_is_live(&context).unwrap();assert!(STORE.lock().frames[&lease.frame].handles.is_empty());assert_eq!(std::fs::read_dir(directory.path().join("inputs")).unwrap().count(),0);
}
#[test]
fn stale_registration_and_cleanup_cannot_affect_a_new_document_in_the_same_namespace(){
    let (directory,provider,registry,lease)=install();let old=frame_context(provider.id(),&lease.frame).unwrap();revoke_frame(provider.id(),&lease.frame);
    let fresh=reregister_frame(&provider,&registry,crate::view_host::ViewCallContext::panel(provider.id()),&lease.frame).unwrap();
    let handle=register_at_root(&mut STORE.lock(),provider.id(),&lease.frame,directory.path(),MediaFile{path:"clip.mp4".into(),content_type:MediaType::Mp4,file_name:None},Instant::now()).unwrap();
    assert!(register_context_media(provider.id(),&lease.frame,&old,MediaFile{path:"clip.mp4".into(),content_type:MediaType::Mp4,file_name:None}).is_err());
    revoke_context_media(provider.id(),&lease.frame,&old,&handle.handle);context_is_live(&fresh.context).unwrap();assert!(open_frame_media(provider.id(),&lease.frame,&handle.handle).is_ok());
}
