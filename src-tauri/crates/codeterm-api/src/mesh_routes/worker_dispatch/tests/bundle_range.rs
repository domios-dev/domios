use base64::Engine;

use super::{bundle_fixture, create_lane_bundle_with_limit, git, MAX_LANE_BUNDLE_BYTES};

#[test]
fn worker_lane_bundle_omits_shared_history_and_keeps_the_lane_ref() {
    for unchanged_lane in [false, true] {
        let (state, mut request, tip, root, _fixture) = bundle_fixture("peer-origin");
        let repo = root.path().join("repo");
        let prerequisite = request.base_commit.clone();
        if unchanged_lane {
            request.base_commit = tip.clone();
        }
        let receiver = root.path().join("receiver");
        std::fs::create_dir_all(&receiver).unwrap();
        git(&receiver, &["init", "-q"]);
        git(
            &receiver,
            &[
                "fetch",
                "--no-tags",
                repo.to_str().unwrap(),
                &request.base_commit,
            ],
        );
        let reference = format!("refs/heads/{}", request.branch);
        let bundle_file = root.path().join("lane.bundle");
        let bundle_path = bundle_file.to_str().unwrap();
        let response = create_lane_bundle_with_limit(
            state,
            "peer-origin".into(),
            request,
            MAX_LANE_BUNDLE_BYTES,
        )
        .unwrap()
        .0;
        let bundle = base64::engine::general_purpose::STANDARD
            .decode(response["bundle"].as_str().unwrap())
            .unwrap();
        std::fs::write(&bundle_file, &bundle).unwrap();
        let (header, _) =
            bundle.split_at(bundle.windows(2).position(|pair| pair == b"\n\n").unwrap());
        let header = std::str::from_utf8(header).unwrap();
        assert!(header
            .lines()
            .any(|entry| entry.starts_with(&format!("-{prerequisite} "))));
        assert_eq!(
            git(&receiver, &["bundle", "list-heads", bundle_path]),
            format!("{tip} {reference}")
        );
        git(&receiver, &["bundle", "verify", bundle_path]);
        git(&receiver, &["fetch", "--no-tags", bundle_path, &reference]);
        assert_eq!(git(&receiver, &["rev-parse", "FETCH_HEAD"]), tip);
    }
}
