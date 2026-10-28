use super::*;
use async_trait::async_trait;
use serde_json::Value;
struct Provider(String);
#[async_trait]
impl ViewProvider for Provider {
    fn id(&self)->&str { &self.0 }
    fn media_delivery(&self)->Option<MediaDeliveryDescriptor> { Some(MediaDeliveryDescriptor { types: vec![MediaType::Mp4, MediaType::Png] }) }
    async fn call(&self,_:&str,_:Value)->Result<Value,crate::view_host::ViewError> { Ok(Value::Null) }
}
fn fixture() -> (tempfile::TempDir, MediaStore, Arc<dyn ViewProvider>, String, Instant, ViewRegistryHandle) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("clip.mp4"), b"\0\0\0\x18ftypisom0123456789").unwrap();
    let provider: Arc<dyn ViewProvider> = Arc::new(Provider(format!("media-fixture-{}",uuid::Uuid::new_v4().simple())));
    let registry = Arc::new(ViewRegistry::new());
    registry.register(provider.id(), provider.clone());
    let frame = token();
    let now = Instant::now();
    let mut store = MediaStore::default();
    store.frames.insert(frame.clone(), Frame { plugin: provider.id().into(), provider: Arc::downgrade(&provider), registry: Arc::downgrade(&registry), expires: now + FRAME_TTL, handles: HashMap::new() });
    (dir, store, provider, frame, now, registry)
}
fn source(path: &str) -> MediaFile { MediaFile { path:path.into(), content_type:MediaType::Mp4, file_name:Some("clip.exe".into()) } }
#[test]
fn registration_is_plugin_and_frame_scoped_and_bounded() {
    let (dir, mut store, provider, frame, now, registry) = fixture();
    let registered = register_at_root(&mut store, provider.id(), &frame, dir.path(), source("clip.mp4"), now).unwrap();
    assert!(valid_token(&registered.handle));
    assert_eq!(registered.file_name, "clip.mp4");
    assert_eq!(registered.url, Some(format!("/api/plugins/{}/media/{frame}/{}", provider.id(), registered.handle)));
    assert_eq!(store.entry("foreign", &frame, &registered.handle, now).err(), Some(MediaRefusal::Foreign));
    assert_eq!(store.entry(provider.id(), &token(), &registered.handle, now).err(), Some(MediaRefusal::Unknown));
    assert_eq!(store.entry(provider.id(), &frame, &registered.handle, now + HANDLE_TTL).err(), Some(MediaRefusal::Expired));
    assert_eq!(store.frame(provider.id(), &frame, now + FRAME_TTL).err(), Some(MediaRefusal::Expired));
    for _ in 1..MAX_HANDLES { register_at_root(&mut store, provider.id(), &frame, dir.path(), source("clip.mp4"), now).unwrap(); }
    assert!(register_at_root(&mut store, provider.id(), &frame, dir.path(), source("clip.mp4"), now).is_err());
    store.cleanup(now + HANDLE_TTL);
    assert!(store.frames[&frame].handles.is_empty());
    registry.unregister(provider.id());
    drop(provider);
    store.cleanup(now);
    assert!(store.frames.is_empty());
}
#[test]
fn registration_rejects_paths_nonfiles_active_mime_and_oversized_files() {
    let (dir, mut store, provider, frame, now, _registry) = fixture();
    std::fs::write(dir.path().join("active.mp4"), b"<html>active</html>").unwrap();
    std::fs::write(dir.path().join("empty.mp4"), []).unwrap();
    std::fs::create_dir(dir.path().join("folder")).unwrap();
    let huge = File::create(dir.path().join("huge.mp4")).unwrap();
    huge.set_len(MAX_MEDIA_BYTES+1).unwrap();
    drop(huge);
    for path in ["../clip.mp4", "/clip.mp4", "C:/clip.mp4", "folder", "active.mp4", "empty.mp4", "huge.mp4", "clip.mp4:stream"] {
        assert!(register_at_root(&mut store, provider.id(), &frame, dir.path(), source(path), now).is_err(), "{path}");
    }
    assert!(serde_json::from_str::<MediaType>("\"image/svg+xml\"").is_err());
    assert!(serde_json::from_str::<MediaType>("\"text/html\"").is_err());
}
#[test]
fn passive_images_and_safe_display_names_are_supported_without_playback() {
    let (dir, mut store, provider, frame, now, _registry) = fixture();
    std::fs::write(dir.path().join("image.png"), b"\x89PNG\r\n\x1a\nrest").unwrap();
    let registered = register_at_root(&mut store, provider.id(), &frame, dir.path(), MediaFile { path:"image.png".into(), content_type:MediaType::Png, file_name:Some("NUL.exe".into()) }, now).unwrap();
    assert!(registered.url.is_none());
    assert_eq!(registered.content_type,"image/png");
    assert_eq!(registered.file_name,"media.png");
    for name in ["CON", "PRN.exe", "CON.foo.exe", "COM¹.exe", "LPT²", "LPT9.txt", "..", ""] { assert_eq!(format::safe_name(name,MediaType::Png),"media.png"); }
    assert_eq!(format::safe_name("../a?b.exe", MediaType::Png), "_a_b.png");
}
#[test]
fn playback_rechecks_revocation_and_reads_independent_ranges() {
    let (dir, store, provider, frame, now, _registry) = fixture();
    STORE.lock().frames.extend(store.frames);
    let registered = register_at_root(&mut STORE.lock(), provider.id(), &frame, dir.path(), source("clip.mp4"), now).unwrap();
    let reader = playback(provider.id(), &frame, &registered.handle).unwrap();
    assert_eq!(reader.read(4,4).unwrap(), b"ftyp");
    assert_eq!(reader.clone().read(8,4).unwrap(), b"isom");
    assert_eq!(reader.read(4,4).unwrap(), b"ftyp");
    assert!(reader.read(0,65537).is_err());
    assert!(frame_owns_handle(provider.id(), &frame, &registered.handle).is_ok());
    assert_eq!(frame_owns_handle(provider.id(), &frame, &token()), Err(MediaRefusal::Foreign));
    assert!(open_frame_media(provider.id(), &frame, &registered.handle).is_ok());
    let opened = open_plugin_media(provider.id(), &registered.handle).unwrap();
    assert!(opened.path.starts_with(dir.path().canonicalize().unwrap()));
    assert_eq!(open_plugin_media("other", &registered.handle).err(), Some(MediaRefusal::Foreign));
    revoke_frame("other", &frame);
    assert!(reader.read(4,4).is_ok());
    revoke_frame(provider.id(), &frame);
    assert!(reader.read(4,4).is_err());
    assert!(open_plugin_media(provider.id(), &registered.handle).is_err());
}
#[cfg(unix)]
#[test]
fn registration_and_resolution_refuse_symlinks_and_replaced_roots() {
    use std::os::unix::fs::symlink;
    let (dir, store, provider, frame, now, _registry) = fixture();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("clip.mp4"),b"\0\0\0\x18ftypisom").unwrap();
    symlink(outside.path().join("clip.mp4"),dir.path().join("link.mp4")).unwrap();
    symlink(outside.path(),dir.path().join("escape")).unwrap();
    STORE.lock().frames.extend(store.frames);
    for path in ["link.mp4","escape/clip.mp4"] { assert!(register_at_root(&mut STORE.lock(),provider.id(),&frame,dir.path(),source(path),now).is_err()); }
    let registered = register_at_root(&mut STORE.lock(),provider.id(),&frame,dir.path(),source("clip.mp4"),now).unwrap();
    let reader = playback(provider.id(),&frame,&registered.handle).unwrap();
    std::fs::remove_file(dir.path().join("clip.mp4")).unwrap();
    symlink(outside.path().join("clip.mp4"),dir.path().join("clip.mp4")).unwrap();
    assert!(open_plugin_media(provider.id(),&registered.handle).is_err());
    assert_eq!(reader.read(4,4).unwrap(),b"ftyp");
    std::fs::write(dir.path().join("safe.mp4"),b"\0\0\0\x18ftypisom").unwrap();
    let alias=outside.path().join("root-link");
    symlink(dir.path(),&alias).unwrap();
    assert!(register_at_root(&mut STORE.lock(),provider.id(),&frame,&alias,source("safe.mp4"),now).is_err());
    revoke_frame(provider.id(),&frame);
}
#[cfg(windows)]
#[test]
fn registration_refuses_windows_reparse_roots() {
    use std::os::windows::fs::symlink_dir;
    let (dir,mut store,provider,frame,now,_registry)=fixture();
    let parent=tempfile::tempdir().unwrap();
    let alias=parent.path().join("root-link");
    // Windows symlink fixtures require Developer Mode or the symlink privilege.
    if symlink_dir(dir.path(),&alias).is_err() { return; }
    assert!(register_at_root(&mut store,provider.id(),&frame,&alias,source("clip.mp4"),now).is_err());
}

#[test]
fn frame_and_plugin_revocation_prevent_deferred_registration_and_old_streams() {
    let (dir, store, provider, frame, now,registry)=fixture();
    STORE.lock().frames.extend(store.frames);
    let first=register_at_root(&mut STORE.lock(),provider.id(),&frame,dir.path(),source("clip.mp4"),now).unwrap();
    let second_frame=create_frame(&provider,&registry).unwrap();
    assert_eq!(frame_owns_handle(provider.id(),&second_frame,&first.handle),Err(MediaRefusal::Foreign));
    assert_eq!(open_frame_media(provider.id(),&second_frame,&first.handle).err(),Some(MediaRefusal::Foreign));
    let reader=playback(provider.id(),&frame,&first.handle).unwrap();
    revoke_plugin(provider.id());
    assert!(reader.read(0,4).is_err());
    assert!(register_at_root(&mut STORE.lock(),provider.id(),&frame,dir.path(),source("clip.mp4"),now).is_err());
    assert!(frame_provider(provider.id(),&second_frame).is_err());
}

#[test]
fn native_registry_replacement_and_removal_revoke_live_frame_authority() {
    let (dir,store,provider,frame,now,registry)=fixture();
    STORE.lock().frames.extend(store.frames);
    let handle=register_at_root(&mut STORE.lock(),provider.id(),&frame,dir.path(),source("clip.mp4"),now).unwrap();
    let reader=playback(provider.id(),&frame,&handle.handle).unwrap();
    registry.register(provider.id(),Arc::new(Provider(provider.id().into())));
    assert!(reader.read(0,4).is_err());
    assert!(open_frame_media(provider.id(),&frame,&handle.handle).is_err());
    registry.unregister(provider.id());
    assert!(frame_provider(provider.id(),&frame).is_err());
    revoke_frame(provider.id(),&frame);
}
