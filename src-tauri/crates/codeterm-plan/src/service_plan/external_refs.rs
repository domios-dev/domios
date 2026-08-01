use crate::plan::{ExternalRef, ExternalRefKind, ExternalRefRelationship};

use super::PlanError;

const GITHUB_PREFIX: &str = "https://github.com/";

pub fn canonicalize_external_ref(raw: &str) -> Result<ExternalRef, PlanError> {
    canonicalize_external_ref_with_relationship(raw, None)
}

pub fn canonicalize_external_ref_with_relationship(
    raw: &str,
    relationship: Option<ExternalRefRelationship>,
) -> Result<ExternalRef, PlanError> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(invalid("reference is empty"));
    }
    let (repository, number) = if value.contains("://") {
        parse_github_url(value)?
    } else {
        parse_repo_number(value)?
    };
    Ok(ExternalRef {
        kind: ExternalRefKind::GithubIssue,
        canonical_url: format!("{GITHUB_PREFIX}{repository}/issues/{number}"),
        repository,
        number,
        relationship,
    })
}

pub fn canonicalize_external_refs(values: &[String]) -> Result<Vec<ExternalRef>, PlanError> {
    let mut refs = Vec::with_capacity(values.len());
    for value in values {
        let external_ref = canonicalize_external_ref(value)?;
        if !refs
            .iter()
            .any(|existing| same_issue(existing, &external_ref))
        {
            refs.push(external_ref);
        }
    }
    Ok(refs)
}

pub fn merge_external_refs(existing: &mut Vec<ExternalRef>, additions: Vec<ExternalRef>) {
    for addition in additions {
        if !existing.iter().any(|item| same_issue(item, &addition)) {
            existing.push(addition);
        }
    }
}

fn parse_github_url(value: &str) -> Result<(String, u64), PlanError> {
    let Some(rest) = strip_prefix_ignore_ascii_case(value, GITHUB_PREFIX) else {
        return Err(invalid(
            "URL must use https://github.com/<owner>/<repository>/issues/<number>",
        ));
    };
    if rest.contains(['?', '#']) {
        return Err(invalid(
            "GitHub issue URL must not contain a query or fragment",
        ));
    }
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    let parts = rest.split('/').collect::<Vec<_>>();
    if parts.len() != 4 || parts[2] != "issues" {
        return Err(invalid(
            "GitHub issue URL path must be <owner>/<repository>/issues/<number>",
        ));
    }
    let repository = canonical_repository(parts[0], parts[1])?;
    let number = parse_number(parts[3])?;
    Ok((repository, number))
}

fn parse_repo_number(value: &str) -> Result<(String, u64), PlanError> {
    let Some((repository, number)) = value.split_once('#') else {
        return Err(invalid(
            "reference must be a GitHub issue URL or <owner>/<repository>#<number>",
        ));
    };
    if number.contains('#') {
        return Err(invalid("reference contains more than one '#' separator"));
    }
    let parts = repository.split('/').collect::<Vec<_>>();
    if parts.len() != 2 {
        return Err(invalid(
            "repository must contain exactly one '/' as <owner>/<repository>",
        ));
    }
    let repository = canonical_repository(parts[0], parts[1])?;
    let number = parse_number(number)?;
    Ok((repository, number))
}

fn canonical_repository(owner: &str, repository: &str) -> Result<String, PlanError> {
    if owner.is_empty() {
        return Err(invalid("repository owner is missing"));
    }
    if repository.is_empty() {
        return Err(invalid("repository name is missing"));
    }
    if !owner
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(invalid(
            "repository owner may contain only ASCII letters, digits, and '-'",
        ));
    }
    if !repository
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(invalid(
            "repository name may contain only ASCII letters, digits, '-', '_', and '.'",
        ));
    }
    Ok(format!(
        "{}/{}",
        owner.to_ascii_lowercase(),
        repository.to_ascii_lowercase()
    ))
}

fn parse_number(value: &str) -> Result<u64, PlanError> {
    if value.is_empty() {
        return Err(invalid("issue number is missing"));
    }
    if !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid("issue number must contain only digits"));
    }
    let number = value
        .parse::<u64>()
        .map_err(|_| invalid("issue number is too large"))?;
    if number == 0 {
        return Err(invalid("issue number must be greater than zero"));
    }
    Ok(number)
}

fn same_issue(left: &ExternalRef, right: &ExternalRef) -> bool {
    left.kind == right.kind && left.repository == right.repository && left.number == right.number
}

fn strip_prefix_ignore_ascii_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    value
        .get(..prefix.len())
        .filter(|candidate| candidate.eq_ignore_ascii_case(prefix))
        .map(|_| &value[prefix.len()..])
}

fn invalid(message: impl Into<String>) -> PlanError {
    PlanError::InvalidExternalRef(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, Plan};
    use crate::service_plan::{NewTask, PlanStore};

    #[test]
    fn external_ref_url_and_repo_number_canonicalize_identically() {
        let url =
            canonicalize_external_ref("https://github.com/RollaCode/CodeTerm-Releases/issues/017/")
                .unwrap();
        let shorthand = canonicalize_external_ref("rollacode/codeterm-releases#17").unwrap();
        assert_eq!(url, shorthand);
        assert_eq!(url.repository, "rollacode/codeterm-releases");
        assert_eq!(url.number, 17);
        assert_eq!(
            url.canonical_url,
            "https://github.com/rollacode/codeterm-releases/issues/17"
        );
    }

    #[test]
    fn external_ref_rejects_malformed_and_ambiguous_inputs() {
        let malformed = canonicalize_external_ref("not-a-ref").unwrap_err();
        assert!(malformed.to_string().contains("must be a GitHub issue URL"));
        let ambiguous = canonicalize_external_ref("#17").unwrap_err();
        assert!(ambiguous.to_string().contains("exactly one '/'"));
        let wrong_path = canonicalize_external_ref("https://github.com/a/b/pull/17").unwrap_err();
        assert!(wrong_path.to_string().contains("URL path"));
    }

    #[test]
    fn external_ref_relationship_serializes_as_data() {
        let item = canonicalize_external_ref_with_relationship(
            "rollacode/codeterm-releases#17",
            Some(ExternalRefRelationship::Fixes),
        )
        .unwrap();
        let json = serde_json::to_value(&item).unwrap();
        assert_eq!(json["kind"], "github_issue");
        assert_eq!(json["relationship"], "fixes");
    }

    #[test]
    fn external_ref_round_trips_and_survives_task_updates_without_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "External refs".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("owner"),
        );
        store.save(&mut plan, 1).unwrap();
        let values = vec![
            "https://github.com/RollaCode/CodeTerm-Releases/issues/017".to_string(),
            "rollacode/codeterm-releases#17".to_string(),
        ];
        let task_id = store
            .add_task_with_external_refs(
                &plan.id,
                NewTask {
                    name: "Linked task".into(),
                    kind: "implement".into(),
                    ..Default::default()
                },
                &values,
                2,
            )
            .unwrap();
        store
            .add_task_external_refs(&plan.id, &task_id, &values, 3)
            .unwrap();
        store
            .update_task(
                &plan.id,
                &task_id,
                Some("Renamed task"),
                None,
                None,
                None,
                Some("blocked"),
                Some(Some("external dependency")),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                &[],
                None,
                None,
                4,
            )
            .unwrap();
        store
            .with_plan(&plan.id, 5, |plan| {
                plan.tasks[0].external_refs[0].relationship =
                    Some(ExternalRefRelationship::RelatesTo);
                Ok(())
            })
            .unwrap();

        store.plans.write().clear();
        store.reload_from_disk(&plan.id).unwrap();
        let task = store.show_task(&task_id).unwrap().task;
        assert_eq!(task.name, "Renamed task");
        assert_eq!(task.external_refs.len(), 1);
        assert_eq!(
            task.external_refs[0].relationship,
            Some(ExternalRefRelationship::RelatesTo)
        );
        assert_eq!(
            task.external_refs[0].canonical_url,
            "https://github.com/rollacode/codeterm-releases/issues/17"
        );
    }
}
