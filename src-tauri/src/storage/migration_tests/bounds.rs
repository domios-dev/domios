//! storage_migration_ bounds: the accepted repository bounds refuse the whole import with
//! the limit and the observation, and an import exactly at a bound is kept whole.

use super::*;

fn many_jobs(count: usize) -> String {
    let jobs: Vec<String> = (0..count)
        .map(|index| {
            format!(
                r#"{{"id": "job-{index}", "name": "n", "workspace_id": "ws", "schedule": "0 * * * *", "enabled": true, "agent": "claude", "task": "t", "created_at": 1}}"#
            )
        })
        .collect();
    format!(
        r#"{{"version": "1", "jobs": [{}], "history": {{}}}}"#,
        jobs.join(",")
    )
}

fn plan_refusal(f: &Fixture, decoder: &Fixed) -> ImportRefusal {
    let error = ImportPlan::resolve(f.capture(), decoder, &BTreeMap::new())
        .expect_err("over a repository bound");
    refusal(error)
}

fn cron_only(jobs: Vec<CronJobImport>, histories: Vec<CronHistoryImport>) -> Fixed {
    Fixed::cron(CronImport { jobs, histories })
}

/// Hands every domain a cron import, as a mismatched decoder would.
struct Swapped;

impl LegacyDecoder for Swapped {
    fn decode(&self, _domain: ImportDomain, _bytes: &[u8]) -> Result<DomainImport, DecodeFailure> {
        Ok(DomainImport::Cron(CronImport::default()))
    }
}

#[test]
fn storage_migration_cron_over_bound_source_refuses_whole_import() {
    let f = Fixture::seeded();
    f.put("crons.json", many_jobs(MAX_CRON_JOBS + 1).as_bytes());
    let originals = tree(&f.source());
    let storage = f.open();

    let error = storage
        .import_legacy_root(&f.request(), &LegacyCodecs)
        .expect_err("over the live job bound");

    assert!(matches!(
        refusal(error),
        ImportRefusal::Decode {
            domain: ImportDomain::Cron,
            status: SourceStatus::Conflict(_),
            ..
        }
    ));
    assert_untouched(&storage);
    assert!(
        !f.backup().join("import.manifest").exists(),
        "nothing was planned"
    );
    assert_eq!(tree(&f.source()), originals);
    close(storage);
}

#[test]
fn storage_migration_cron_bounds_refuse_with_limit_and_observed() {
    let f = Fixture::seeded();
    let jobs = (0..=MAX_CRON_JOBS)
        .map(|index| job(&format!("job-{index}")))
        .collect();
    assert_eq!(
        plan_refusal(&f, &cron_only(jobs, Vec::new())),
        ImportRefusal::Bound {
            domain: ImportDomain::Cron,
            bound: "jobs",
            limit: MAX_CRON_JOBS as u64,
            observed: MAX_CRON_JOBS as u64 + 1,
        }
    );

    let history = CronHistoryImport {
        job_id: CronJobId::new("busy").expect("job id"),
        runs: (0..=MAX_CRON_HISTORY)
            .map(|index| run(&format!("run-{index}"), index))
            .collect(),
    };
    assert_eq!(
        plan_refusal(&f, &cron_only(vec![job("busy")], vec![history])),
        ImportRefusal::Bound {
            domain: ImportDomain::Cron,
            bound: "history-per-job",
            limit: MAX_CRON_HISTORY,
            observed: MAX_CRON_HISTORY + 1,
        }
    );

    let mut wide = job("wide");
    wide.record.max_history = MAX_CRON_HISTORY + 1;
    assert_eq!(
        plan_refusal(&f, &cron_only(vec![wide], Vec::new())),
        ImportRefusal::Bound {
            domain: ImportDomain::Cron,
            bound: "max-history",
            limit: MAX_CRON_HISTORY,
            observed: MAX_CRON_HISTORY + 1,
        }
    );

    let mut long = job("long");
    long.record.task = "x".repeat(1024 * 1024 + 1);
    assert!(matches!(
        plan_refusal(&f, &cron_only(vec![long], Vec::new())),
        ImportRefusal::Invalid {
            domain: ImportDomain::Cron,
            ..
        }
    ));

    assert!(
        !f.backup().join("import.manifest").exists(),
        "no plan was published"
    );
    let storage = f.open();
    assert_untouched(&storage);
    close(storage);
}

#[test]
fn storage_migration_bounds_at_the_limit_import_exactly() {
    let f = Fixture::seeded();
    let mut busy = job("busy");
    busy.record.max_history = MAX_CRON_HISTORY;
    let mut jobs: Vec<CronJobImport> = (1..MAX_CRON_JOBS)
        .map(|index| job(&format!("job-{index}")))
        .collect();
    jobs.insert(0, busy);
    let history = CronHistoryImport {
        job_id: CronJobId::new("busy").expect("job id"),
        runs: (0..MAX_CRON_HISTORY)
            .map(|index| run(&format!("run-{index}"), index))
            .collect(),
    };
    let plan = ImportPlan::resolve(
        f.capture(),
        &cron_only(jobs, vec![history]),
        &BTreeMap::new(),
    )
    .expect("exactly at the bounds");
    let storage = f.open();

    let receipt = storage
        .commit_legacy_import(plan, DEADLINE)
        .expect("imported whole");

    let comparison = receipt.comparison.expect("comparison");
    assert!(comparison.is_clean());
    let cron = &comparison.domains[0];
    assert_eq!(
        (cron.committed_entities, cron.committed_items),
        (MAX_CRON_JOBS as u64, MAX_CRON_HISTORY)
    );
    let snapshot = cron_snapshot(&storage);
    assert_eq!(snapshot.states.len(), MAX_CRON_JOBS);
    assert_eq!(
        snapshot.states[0].history.len() as u64,
        MAX_CRON_HISTORY,
        "no run was trimmed"
    );
    close(storage);
}

#[test]
fn storage_migration_sentinel_bounds_refuse_whole_import() {
    let f = Fixture::seeded();
    let over = MAX_SENTINEL_RECORDS + 1;
    assert_eq!(
        plan_refusal(&f, &Fixed::sentinel(stages(over))),
        ImportRefusal::Bound {
            domain: ImportDomain::Sentinel,
            bound: "stages",
            limit: MAX_SENTINEL_RECORDS as u64,
            observed: over as u64,
        }
    );

    let mut repeated = stages(2);
    repeated[1].identity.id = repeated[0].identity.id.clone();
    assert!(matches!(
        plan_refusal(&f, &Fixed::sentinel(repeated)),
        ImportRefusal::Invalid {
            domain: ImportDomain::Sentinel,
            ..
        }
    ));

    let orphan: Vec<SentinelImport> = decoded_sentinels()
        .into_iter()
        .filter(|record| record.identity.id.as_str() == "sentinel_child")
        .collect();
    assert!(matches!(
        plan_refusal(&f, &Fixed::sentinel(orphan)),
        ImportRefusal::Invalid {
            domain: ImportDomain::Sentinel,
            ..
        }
    ));

    let storage = f.open();
    assert_untouched(&storage);
    close(storage);
}

#[test]
fn storage_migration_sentinel_field_limits_refuse_whole_import() {
    let f = Fixture::seeded();
    let mut wide = stages(1);
    wide[0].identity.cwd = "x".repeat(32 * 1024 + 1);

    let refused = plan_refusal(&f, &Fixed::sentinel(wide));

    assert!(
        matches!(
            refused,
            ImportRefusal::Invalid {
                domain: ImportDomain::Sentinel,
                ..
            }
        ),
        "{refused:?}"
    );
    assert!(
        !f.backup().join("import.manifest").exists(),
        "nothing was planned"
    );
    let storage = f.open();
    assert_untouched(&storage);
    close(storage);
}

#[test]
fn storage_migration_wrong_domain_decoder_output_refuses() {
    let f = Fixture::seeded();

    let error = ImportPlan::resolve(f.capture(), &Swapped, &BTreeMap::new())
        .expect_err("a Sentinel original decoded as cron");

    assert_eq!(
        refusal(error),
        ImportRefusal::WrongDomain {
            expected: ImportDomain::Sentinel,
            decoded: ImportDomain::Cron,
        }
    );
}
