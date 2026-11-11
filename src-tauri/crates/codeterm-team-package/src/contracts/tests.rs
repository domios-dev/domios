use std::path::Path;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::*;
use crate::{check_folder, CheckOptions};

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/contracts")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(&path).expect("fixture reads"))
        .expect("fixture is JSON")
}

/// Deserializing and re-serializing must reproduce the fixture exactly, so a renamed,
/// removed or added field on either side fails here before the UI sees it.
fn round_trip<T: Serialize + DeserializeOwned>(value: Value) -> T {
    let typed: T =
        serde_json::from_value(value.clone()).expect("fixture matches the Rust contract");
    assert_eq!(serde_json::to_value(&typed).expect("serializes"), value);
    typed
}

#[derive(Serialize, Deserialize)]
struct ExportExchange {
    request: TemplateExportRequest,
    result: TemplateExportResult,
}

#[test]
fn shared_ui_fixtures_match_the_rust_contracts() {
    round_trip::<TemplatePreview>(fixture("preview.json"));
    round_trip::<TemplateStartRequest>(fixture("start-request.json"));
    round_trip::<Vec<TemplateStartResult>>(fixture("start-results.json"));
    round_trip::<Vec<ModelDecision>>(fixture("model-decisions.json"));
    round_trip::<TemplateRecall>(fixture("recall.json"));
    round_trip::<TemplateList>(fixture("list.json"));
    round_trip::<ExportExchange>(fixture("export.json"));
    round_trip::<PayloadApplyResult>(fixture("payload-apply.json"));
    round_trip::<crate::PackageCheckReport>(fixture("check-report.json"));
    let unavailable: TemplateUnavailable = round_trip(fixture("unavailable.json"));
    assert_eq!(
        unavailable,
        TemplateUnavailable::new(TemplateOperation::Start)
    );
}

#[test]
fn icons_become_base64_data_urls() {
    assert_eq!(base64(b"<svg/>"), "PHN2Zy8+");
    assert_eq!(base64(b"a"), "YQ==");
    assert_eq!(base64(b"ab"), "YWI=");
    assert_eq!(base64(b""), "");
    let icon = TemplateIcon::new(IconKind::Png, b"abc");
    assert_eq!(icon.data_url, "data:image/png;base64,YWJj");
}

#[test]
fn portable_projections_come_from_the_package_alone() {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/corpus/valid/reference");
    let package = check_folder(&folder, &CheckOptions::domios())
        .package
        .expect("reference is valid");

    let summary = TemplateSummary::from_package(&package, TemplateProvenance::Imported);
    assert_eq!(summary.name, "com.example/research-desk");
    assert!(summary
        .icon
        .is_some_and(|icon| icon.data_url.starts_with("data:image/svg+xml;base64,")));

    let items = payload_items(&package);
    let executable: Vec<&str> = items
        .iter()
        .filter(|item| item.executable)
        .map(|item| item.id.as_str())
        .collect();
    assert_eq!(executable, vec!["skill:source-check"]);
    assert!(items
        .iter()
        .any(|item| item.id == "skill:skills/market-sizing" && item.label == "market-sizing"));

    let roles = role_summaries(&package);
    let curator = roles
        .iter()
        .find(|role| role.role == "curator")
        .expect("curator");
    assert_eq!(
        curator.start,
        crate::RoleStart::Schedule {
            schedule: "45m..2h".into()
        }
    );
    let cartographer = roles
        .iter()
        .find(|role| role.role == "cartographer")
        .expect("cartographer");
    assert_eq!(
        cartographer.start,
        crate::RoleStart::Mode(crate::StartMode::OnDemand)
    );
    assert_eq!(roles[0].label, "Lead");

    let inputs = input_fields(&package);
    assert!(inputs[0].required && !inputs[1].required);

    let asks = declared_asks(&package);
    assert!(asks.contains(&TemplateAsk::Script {
        item: "skill:source-check".into()
    }));
    assert!(asks.contains(&TemplateAsk::RepoWrite));
    assert!(!asks.contains(&TemplateAsk::Browser));
}
