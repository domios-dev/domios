use super::fingerprint;

#[test]
fn fingerprint_is_the_first_eight_sha256_hex_digits() {
    assert_eq!(fingerprint(Some("abc")), "ba7816bf");
}

#[test]
fn absent_or_blank_keys_have_no_fingerprint() {
    for key in [None, Some(""), Some(" \t\n")] {
        assert_eq!(fingerprint(key), "none");
    }
}

#[test]
fn different_key_bytes_produce_distinct_fingerprints() {
    assert_ne!(fingerprint(Some("abc")), fingerprint(Some("abd")));
    assert_ne!(fingerprint(Some("abc")), fingerprint(Some(" abc")));
}
