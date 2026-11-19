use super::*;

fn entry(slug: &str, body: &str) -> PlanSkillManifestEntry {
    PlanSkillManifestEntry {
        slug: slug.into(),
        title: "opaque title".into(),
        purpose: "opaque applicability".into(),
        content_digest: format!("sha256:{:x}", Sha256::digest(body.as_bytes())),
    }
}

#[test]
fn remote_brief_carries_exact_skill_snapshot_without_local_plan_dependencies() {
    let body = "---\r\ndescription: opaque applicability\r\n---\r\n\nopaque αβ\n````text\nopaque code\n````";
    let manifest = vec![entry("opaque-skill", body)];
    let mut reads = Vec::new();
    let context = render_skill_context("origin-plan", &manifest, |slug| {
        reads.push(slug.to_owned());
        Ok(body.into())
    })
    .unwrap();
    assert_eq!(reads, ["opaque-skill"]);
    assert!(context.contains(&manifest[0].content_digest));
    let start = context.find("`````markdown\n").unwrap() + "`````markdown\n".len();
    assert_eq!(
        &context.as_bytes()[start..start + body.len()],
        body.as_bytes()
    );
    assert_eq!(&context[start + body.len()..], "\n`````\n\n");

    let task: codeterm_service_core::plan::Task = serde_json::from_value(serde_json::json!({
        "id": "origin-task", "name": "opaque task", "type": "unplanned",
        "status": "pending", "body": "opaque assignment", "personality": "Ada"
    }))
    .unwrap();
    let local = crate::server::plan_task_dispatch::brief::build_brief_with_manifest(
        (&task, &[][..]),
        "origin-plan",
        &[],
        &["opaque gate".into()],
    );
    let brief = remote_worker_brief(
        local,
        "origin-plan",
        "origin-task",
        "origin-attempt",
        "origin-peer",
        "origin-controller",
        &context,
    );
    assert!(brief.contains(&context));
    assert_eq!(brief.matches(body).count(), 1);
    assert!(brief.contains("opaque assignment"));
    assert!(brief.contains("opaque gate"));
    assert!(!brief.contains("domios plan skill show"));
    assert!(!brief.contains("domios agent report"));
    assert!(!brief.contains(codeterm_plan::service_plan::worker_identity::WORKER_ACK_INSTRUCTION));
    let ack =
        "domios plan task remote-ack origin-plan origin-task origin-attempt --mesh origin-peer";
    assert!(brief.find(ack).unwrap() < brief.find("opaque assignment").unwrap());
    assert!(brief.contains("domios plan task remote-report origin-plan origin-task origin-attempt --message-file <report-path> --sha <lane-head> --mesh origin-peer"));
    assert!(brief.contains(
        "domios send --tab origin-controller --mesh origin-peer --from-pad <message.md>"
    ));
}

#[test]
fn remote_skill_snapshot_refuses_changed_missing_and_oversized_bodies() {
    let manifest = vec![entry("opaque", "expected")];
    assert!(render_skill_context("p", &manifest, |_| Ok("changed".into())).is_err());
    assert!(render_skill_context("p", &manifest, |_| Err(invalid_dispatch("missing"))).is_err());
    let body = "x".repeat(MAX_SKILL_CONTEXT_BYTES);
    let manifest = vec![entry("opaque", &body)];
    assert!(render_skill_context("p", &manifest, |_| Ok(body.clone())).is_err());
    assert_eq!(
        render_skill_context("p", &[], |_| panic!("empty manifest must not read skills")).unwrap(),
        ""
    );
}
