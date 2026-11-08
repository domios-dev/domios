use super::*;

#[test]
fn binary_bytes_are_verified_before_materialization() {
    let bytes = b"opaque fixture executable";
    let expected = format!("{:x}", Sha256::digest(bytes));
    assert!(verify(bytes, &expected).is_ok());
    assert!(verify(b"tampered", &expected).is_err());
    assert!(verify(bytes, "").is_err());
    assert!(verify(bytes, &"0".repeat(64)).is_err());
}
