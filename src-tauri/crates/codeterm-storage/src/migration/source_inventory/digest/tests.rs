use super::*;

#[test]
fn sha256_matches_nist_vectors_across_streaming_block_boundaries() {
    let vectors: &[(&[u8], &str)] = &[
        (
            b"",
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            b"abc",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        (
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        ),
    ];
    for (bytes, expected) in vectors {
        assert_eq!(Sha256Digest::of(bytes).to_hex(), *expected);
        for size in [1, 3, 55, 56, 63, 64, 65] {
            let mut hasher = Sha256::new();
            for chunk in bytes.chunks(size) {
                hasher.update(chunk);
            }
            assert_eq!(hasher.finish().to_hex(), *expected);
        }
    }
    let bytes = vec![b'a'; 1_000_000];
    assert_eq!(
        Sha256Digest::of(&bytes).to_hex(),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
}
