use super::*;
struct Temp(std::path::PathBuf);
impl Temp {
    fn new()->Self {
        let path=std::env::temp_dir().join(format!("plugin-media-open-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Temp { fn drop(&mut self) { let _=std::fs::remove_dir_all(&self.0); } }
#[test]
fn confined_binary_open_preserves_bounded_text_reads_and_rejects_nonfiles() {
    let root=Temp::new();
    std::fs::create_dir(root.0.join("nested")).unwrap();
    std::fs::write(root.0.join("nested/file.txt"),b"opaque").unwrap();
    let mut file=open_file(&root.0,"nested/file.txt").unwrap();
    let mut bytes=Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes,b"opaque");
    assert_eq!(read_bounded(&root.0,"nested/file.txt",6).unwrap(),"opaque");
    assert_eq!(read_bounded(&root.0,"nested/file.txt",5),Err("budget_exhausted"));
    for path in ["", "../file.txt", "/file.txt", "nested"] { assert!(open_file(&root.0,path).is_err(),"{path}"); }
    std::fs::write(root.0.join("binary"),[0xff,0xfe]).unwrap();
    assert!(open_file(&root.0,"binary").is_ok());
    assert!(read(&root.0,"binary").is_err());
}
#[cfg(unix)]
#[test]
fn binary_open_denies_child_and_root_symlinks_while_text_wrapper_keeps_root_alias_behavior() {
    use std::os::unix::fs::symlink;
    let root=Temp::new();
    let aliases=Temp::new();
    std::fs::write(root.0.join("source"),b"opaque").unwrap();
    symlink(root.0.join("source"),root.0.join("child")).unwrap();
    let alias=aliases.0.join("root");
    symlink(&root.0,&alias).unwrap();
    assert!(open_file(&root.0,"child").is_err());
    assert!(open_file(&alias,"source").is_err());
    assert_eq!(read(&alias,"source").unwrap(),"opaque");
}
