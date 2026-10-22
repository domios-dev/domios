use sha2::{Digest, Sha256};

#[test]
fn known_artifact_digests_match_for_every_read_boundary() {
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    assert!(std::arch::is_aarch64_feature_detected!("sha2"));

    println!(
        "artifact_sha256 force_soft={} target={}-{}",
        cfg!(feature = "sha2-software"),
        std::env::consts::ARCH,
        std::env::consts::OS
    );
    let cases = [
        (
            Vec::new(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            b"a".to_vec(),
            "ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb",
        ),
        (
            b"abc".to_vec(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        (
            patterned_bytes(55),
            "463eb28e72f82e0a96c0a4cc53690c571281131f672aa229e0d45ae59b598b59",
        ),
        (
            patterned_bytes(56),
            "da2ae4d6b36748f2a318f23e7ab1dfdf45acdc9d049bd80e59de82a60895f562",
        ),
        (
            patterned_bytes(63),
            "29af2686fd53374a36b0846694cc342177e428d1647515f078784d69cdb9e488",
        ),
        (
            patterned_bytes(64),
            "fdeab9acf3710362bd2658cdc9a29e8f9c757fcf9811603a8c447cd1d9151108",
        ),
        (
            patterned_bytes(65),
            "4bfd2c8b6f1eec7a2afeb48b934ee4b2694182027e6d0fc075074f2fabb31781",
        ),
        (
            patterned_bytes(127),
            "92ca0fa6651ee2f97b884b7246a562fa71250fedefe5ebf270d31c546bfea976",
        ),
        (
            patterned_bytes(128),
            "471fb943aa23c511f6f72f8d1652d9c880cfa392ad80503120547703e56a2be5",
        ),
        (
            patterned_bytes(129),
            "5099c6a56203f9687f7d33f4bfdf576d31dc91f6b695ecea38b2770c87631135",
        ),
        (
            patterned_bytes(65_537),
            "237356e18b503616912abb8ffaed3a72591e397d4ac294c4637917d48a3f529d",
        ),
        (
            patterned_bytes(3 * 1024 * 1024 + 129),
            "92e5e91b4a5fefac4524d5759e18e58bfaf75f782f9f6fec1b0146300f8ec8dd",
        ),
    ];
    for (input, expected) in cases {
        assert_eq!(format!("{:x}", Sha256::digest(&input)), expected);
        let boundaries: &[usize] = if input.len() <= 65_537 {
            &[1, 31, 64, 16_384, 65_537]
        } else {
            &[31, 64, 16_384, 65_537]
        };
        for &boundary in boundaries {
            let mut hash = Sha256::new();
            for chunk in input.chunks(boundary) {
                hash.update(chunk);
            }
            assert_eq!(
                format!("{:x}", hash.finalize()),
                expected,
                "bytes={} read_boundary={boundary}",
                input.len()
            );
        }
        println!("artifact_sha256 bytes={} digest={expected}", input.len());
    }
}

fn patterned_bytes(length: usize) -> Vec<u8> {
    (0..length).map(|index| (index % 251) as u8).collect()
}
