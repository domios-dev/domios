use super::*;
fn probe(resource: &str) -> GithubProbe { serde_json::from_value(json!({"repo":"owner/repo","resource":resource,"id":42})).unwrap() }

#[test]
fn github_reports_failing_before_completion_with_failed_and_timed_out_job_names() {
    for resource in ["run", "jobs"] {
        let reading = github_reading(&probe(resource), json!({"status":"in_progress", "conclusion":"", "url":"https://example.com/run", "jobs":[
            {"name":"Linux", "conclusion":"failure"}, {"name":"Windows", "conclusion":"timed_out"}, {"name":"macOS", "conclusion":"success"}
        ]})).unwrap().unwrap();
        assert_eq!(reading.state, "failing");
        assert_eq!(reading.output["failed_jobs"], json!(["Linux", "Windows"]));
        assert_eq!(reading.output["status"], "in_progress");
        assert!(reading.summary.contains("Windows"));
    }
    for (status, conclusion) in [("in_progress", ""), ("completed", "failure"), ("completed", "success")] {
        let reading = github_reading(&probe("run"), json!({"status":status,"conclusion":conclusion,"jobs":[]})).unwrap().unwrap();
        assert_eq!(reading.state, "done");
        assert_eq!(reading.output["conclusion"], conclusion);
    }
    assert!(github_reading(&probe("pr_checks"), json!({"statusCheckRollup":[]})).unwrap().is_none());
    assert!(github_reading(&probe("run"), json!({"error":"denied"})).is_err());
}

#[test]
fn github_runner_reading_counts_label_matches_across_pages_and_busy_is_online() {
    let probe: GithubProbe = serde_json::from_value(json!({"org":"owner","resource":"runners","label":"linux-build"})).unwrap();
    let reading = github_reading(&probe, json!([
        {"runners":[{"name":"one","status":"online","busy":true,"labels":[{"name":"Linux-Build"}]}, {"status":"online","labels":[{"name":"other"}]}]},
        {"runners":[{"status":"online","labels":[{"name":"linux-build"}]}, {"status":"offline","labels":[{"name":"linux-build"}]}]}
    ])).unwrap().unwrap();
    assert_eq!(reading.state, "online"); assert_eq!(reading.output["online_count"], 2);
    let reading = github_reading(&probe, json!({"runners":[]})).unwrap().unwrap();
    assert_eq!(reading.state, "offline"); assert_eq!(reading.output["online_count"], 0);
    assert!(github_reading(&probe, json!({"message":"denied"})).is_err());
}
