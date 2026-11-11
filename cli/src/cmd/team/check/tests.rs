use super::render;
use codeterm_team_package::{check_yaml, CheckOptions};

#[test]
fn human_report_names_file_line_code_and_verdict() {
    let text = "name: dev.example/probe\nversion: 1.0.0\ndescription: Probe.\nmanagers:\n  orchestrator:\n    kind: lead\n";
    let report = check_yaml(text, &CheckOptions::domios()).report;
    let rendered = render("probe.team.yaml", &report);
    assert!(
        rendered.contains("probe.team.yaml:6:5: error[removed_field] managers.orchestrator.kind:"),
        "{rendered}"
    );
    assert!(
        rendered.ends_with("invalid dev.example/probe@1.0.0: 1 error(s), 0 warning(s)\n"),
        "{rendered}"
    );
}
