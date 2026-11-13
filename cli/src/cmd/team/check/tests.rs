use super::render;
use codeterm_team_package::{check_yaml, CheckOptions};

#[test]
fn human_report_names_file_line_code_and_verdict() {
    let text = "name: dev.example/probe\nversion: 1.0.0\ndescription: Probe.\nroles:\n  orchestrator:\n    kind: manager\n    persona: Lead\n    instructions: Lead.\n";
    let report = check_yaml(text, &CheckOptions::default()).report;
    let rendered = render("probe.team.yaml", &report);
    assert!(
        rendered.contains("probe.team.yaml:7:5: error[removed_field] roles.orchestrator.persona:"),
        "{rendered}"
    );
    assert!(
        rendered.ends_with("invalid dev.example/probe@1.0.0: 1 error(s), 0 warning(s)\n"),
        "{rendered}"
    );
}
