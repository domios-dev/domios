use super::*;
struct Temp(PathBuf);
impl Temp {fn new()->Self{let path=std::env::temp_dir().join(format!("binary-output-{}",uuid::Uuid::new_v4()));std::fs::create_dir(&path).unwrap();Self(path)}}
impl Drop for Temp{fn drop(&mut self){let _=std::fs::remove_dir_all(&self.0);}}
#[test]
fn partials_cleanup_exclusive_publication_and_retained_bytes_are_exact() {
    let root=Temp::new();std::fs::write(root.0.join("sentinel"),b"preserve").unwrap();
    {let mut output=OwnedOutput::create(&root.0,"Output.bin").unwrap();output.write_all(&[0,255,128,1]).unwrap();}
    assert_eq!(std::fs::read_dir(&root.0).unwrap().count(),1);
    let mut output=OwnedOutput::create(&root.0,"Output.bin").unwrap();output.write_all(&[0,255,128,1]).unwrap();let path=output.publish().unwrap();output.retain();
    assert_eq!(std::fs::read(path).unwrap(),[0,255,128,1]);
    {let mut collision=OwnedOutput::create(&root.0,"sentinel").unwrap();collision.write_all(b"replacement").unwrap();assert!(collision.publish().is_err());}
    assert_eq!(std::fs::read(root.0.join("sentinel")).unwrap(),b"preserve");
    assert_eq!(std::fs::read_dir(&root.0).unwrap().count(),2);
    for name in ["../escape","/absolute","bad:stream",".hidden"]{assert!(OwnedOutput::create(&root.0,name).is_err());}
}
#[cfg(unix)]
#[test]
fn pinned_directory_swap_never_writes_or_cleans_the_replacement_directory() {
    use std::os::unix::fs::symlink;
    let parent=Temp::new();let root=parent.0.join("plugin");let moved=parent.0.join("moved");let outside=parent.0.join("outside");
    std::fs::create_dir(&root).unwrap();std::fs::create_dir(&outside).unwrap();std::fs::write(outside.join("sentinel"),b"preserve").unwrap();
    let mut output=OwnedOutput::create(&root,"Output.bin").unwrap();output.write_all(&[0,255]).unwrap();
    std::fs::rename(&root,&moved).unwrap();symlink(&outside,&root).unwrap();assert!(output.publish().is_err());drop(output);
    assert_eq!(std::fs::read_dir(&moved).unwrap().count(),0);assert_eq!(std::fs::read_dir(&outside).unwrap().count(),1);
    assert!(OwnedOutput::create(&root,"Output.bin").is_err());
}

#[test]
fn child_publication_uses_only_its_owned_directory(){
    let root=Temp::new();let mut output=OwnedOutput::create_child(&root.0,"inputs","input.bin").unwrap();output.write_all(&[0,255]).unwrap();let path=output.publish().unwrap();output.retain();assert_eq!(path,root.0.join("inputs/input.bin"));assert_eq!(std::fs::read(path).unwrap(),[0,255]);
}
#[cfg(unix)]
#[test]
fn child_output_refuses_a_symlink_without_touching_its_target(){
    use std::os::unix::fs::symlink;
    let root=Temp::new();let outside=Temp::new();std::fs::write(outside.0.join("sentinel"),b"preserve").unwrap();symlink(&outside.0,root.0.join("inputs")).unwrap();assert!(OwnedOutput::create_child(&root.0,"inputs","input.bin").is_err());assert_eq!(std::fs::read_dir(&outside.0).unwrap().count(),1);
}
