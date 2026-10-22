use sha2::{Digest, Sha256};

#[test]
fn known_artifact_digests_match_for_every_read_boundary() {
    let cases = [
        (
            Vec::new(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            b"abc".to_vec(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        (
            (0..65_537).map(|index| (index % 251) as u8).collect(),
            "237356e18b503616912abb8ffaed3a72591e397d4ac294c4637917d48a3f529d",
        ),
    ];
    for (input, expected) in cases {
        for boundary in [1, 31, 16_384, 65_537] {
            let mut hash = Sha256::new();
            for chunk in input.chunks(boundary) {
                hash.update(chunk);
            }
            assert_eq!(format!("{:x}", hash.finalize()), expected);
        }
        println!("artifact_sha256 bytes={} digest={expected}", input.len());
    }
}
