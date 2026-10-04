//! storage_cron_ concurrency: admission, fences and publication ordering through the
//! production commit path, sequenced by writer holds and stage gates alone.

use std::time::Duration;

use super::*;
use crate::cron::scheduler::record_fire_start;
use crate::cron::store::storage::CronStage;
use crate::cron::store::{
    commit, finish_run, settle_port, CronCommitError, CronRunStatus, PortSettlement,
};
use crate::mem::dreaming::{ensure_dreaming_cron, DREAMING_CRON_NAME};
use crate::mem::ports::MemSchedulerPort;
use crate::mem_host::CronSchedulerPort;
use crate::service::spawn::CatalogBackgroundModelPort;
use crate::storage::{Storage, StorageConfig};

#[test]
fn storage_cron_held_writer_leaves_cron_lock_and_committed_reads_available() {
    let f = fixture();
    let job = f.create(params("held"));
    let hold = held(&f.storage);
    let submitted = gate(&f.cron, CronStage::Submitted);
    let (cron, id) = (f.cron.clone(), job.id.clone());
    let renamer = std::thread::spawn(move || {
        commit(&cron, |store| {
            store.prepare_update(&id, rename("after hold"))
        })
        .map(|done| done.value.name)
    });
    submitted.wait_reached();
    submitted.release();

    let guard = f
        .cron
        .try_lock()
        .expect("no cron lock is held while a commit waits");
    assert_eq!(guard.get(&job.id).unwrap().name, "held");
    drop(guard);
    let snapshot = f.snapshot();
    let stored = state_of(&snapshot, &job.id).job.as_ref().unwrap();
    assert_eq!(
        stored.record.name, "held",
        "committed reads proceed under a held writer"
    );

    hold.release();
    assert_eq!(renamer.join().unwrap(), Ok("after hold".to_string()));
    assert_eq!(f.name_of(&job.id), "after hold");
    f.assert_cache_is_committed();
}

/// The conflicting delete is committed but not yet published when the run update is
/// admitted, so only the writer's revision check can see it.
#[test]
fn storage_cron_stale_run_update_rereads_and_reprepares() {
    let f = fixture();
    let job = f.create(params("contended"));
    f.start(&job.id, "r1");
    let mut armed = gates(&f.cron, &[CronStage::Prepared, CronStage::Committed]);
    let committed = armed.pop().expect("committed gate");
    let prepared = armed.pop().expect("prepared gate");
    let (cron, id) = (f.cron.clone(), job.id.clone());
    let finisher =
        std::thread::spawn(move || finish_run(&cron, &id, "r1", CronRunStatus::Failed, None));
    prepared.wait_reached();
    let (cron, id) = (f.cron.clone(), job.id.clone());
    let deleter = std::thread::spawn(move || {
        commit(&cron, |store| store.prepare_delete(&id, true)).map(|_| ())
    });
    committed.wait_reached();
    prepared.release();

    assert_eq!(
        finisher.join().unwrap(),
        Ok(false),
        "re-prepared from the reread retired job: no accounting"
    );
    committed.release();
    deleter
        .join()
        .unwrap()
        .expect("another owner retired the job");
    let fresh = f.rehydrated();
    assert!(fresh.get(&job.id).is_none());
    assert_eq!(fresh.get_history(&job.id)[0].status, CronRunStatus::Failed);
    f.assert_cache_is_committed();
}

#[test]
fn storage_cron_publication_follows_commit() {
    let f = fixture();
    let job = f.create(params("ordered"));
    let committed = gate(&f.cron, CronStage::Committed);
    let (cron, id) = (f.cron.clone(), job.id.clone());
    let renamer = std::thread::spawn(move || {
        commit(&cron, |store| {
            store.prepare_update(&id, rename("published"))
        })
        .map(|_| ())
    });
    committed.wait_reached();
    let snapshot = f.snapshot();
    let stored = state_of(&snapshot, &job.id).job.as_ref().unwrap();
    assert_eq!(
        stored.record.name, "published",
        "durable before publication"
    );
    assert_eq!(
        f.name_of(&job.id),
        "ordered",
        "the cache waits for its publication"
    );
    committed.release();
    renamer.join().unwrap().expect("rename");
    assert_eq!(f.name_of(&job.id), "published");
}

#[test]
fn storage_cron_shutdown_settles_admitted_work_and_refuses_late_work() {
    let f = fixture();
    let job = f.create(params("draining"));
    let hold = held(&f.storage);
    let submitted = gate(&f.cron, CronStage::Submitted);
    let (cron, id) = (f.cron.clone(), job.id.clone());
    let admitted = std::thread::spawn(move || record_fire_start(&cron, &id, "admitted-run"));
    submitted.wait_reached();
    let shutdown = f.storage.shutdown();
    submitted.release();

    assert!(
        !record_fire_start(&f.cron, &job.id, "late-run"),
        "a run refused at admission never dispatches"
    );
    hold.release();
    assert!(admitted.join().unwrap(), "admitted work drains and commits");
    shutdown.wait(DEADLINE).expect("shutdown settles");
    let last = f
        .cron
        .lock()
        .get(&job.id)
        .unwrap()
        .last_run
        .clone()
        .unwrap();
    assert_eq!(last.run_id, "admitted-run");

    let reopened = Storage::open(instance(&f.temp), StorageConfig::default()).expect("reopen");
    let fresh = hydrate(&reopened, &f.temp, &f.clock);
    let cache = f.cron.lock();
    assert_eq!(fresh.file.jobs, cache.file.jobs);
    assert_eq!(histories(&fresh), histories(&cache));
}

#[test]
fn storage_cron_backpressure_refuses_without_claim() {
    let f = fixture_with(StorageConfig {
        mailbox_commands: 1,
        ..StorageConfig::default()
    });
    let job = f.create(params("bounded"));
    let hold = held(&f.storage);
    let submitted = gate(&f.cron, CronStage::Submitted);
    let (cron, id) = (f.cron.clone(), job.id.clone());
    let queued = std::thread::spawn(move || record_fire_start(&cron, &id, "queued-run"));
    submitted.wait_reached();
    submitted.release();

    let refused = commit(&f.cron, |store| {
        store.prepare_record_run(&job.id, running("refused-run"))
    });
    assert!(
        matches!(&refused, Err(CronCommitError::NotApplied(_))),
        "{refused:?}"
    );
    assert!(
        !refused.unwrap_err().applied(),
        "a refused admission claims nothing"
    );
    hold.release();
    assert!(queued.join().unwrap());
    let history = f.cron.lock().get_history(&job.id).len();
    assert_eq!(history, 1, "only the admitted run was recorded");
    f.assert_cache_is_committed();
}

#[test]
fn storage_cron_unconfirmed_commit_fences_changes_until_reconciled() {
    let f = fixture();
    let job = f.create(params("unconfirmed"));
    let before = f.storage.revision();
    f.cron.lock().set_storage_deadline(Duration::from_millis(1));
    let hold = held(&f.storage);

    let outcome = commit(&f.cron, |store| {
        store.prepare_update(&job.id, rename("pending rename"))
    });
    assert!(
        matches!(outcome, Err(CronCommitError::Unconfirmed(_))),
        "{outcome:?}"
    );
    assert_eq!(
        f.name_of(&job.id),
        "unconfirmed",
        "nothing unconfirmed is published"
    );
    let fenced = commit(&f.cron, |store| store.prepare_set_enabled(&job.id, false));
    assert!(
        matches!(fenced, Err(CronCommitError::Fenced(_))),
        "{fenced:?}"
    );

    f.cron.lock().set_storage_deadline(DEADLINE);
    hold.release();
    f.storage
        .committed_after(before)
        .wait(DEADLINE)
        .expect("the earlier submission commits on its own");
    let disabled = commit(&f.cron, |store| store.prepare_set_enabled(&job.id, false))
        .expect("the fence settles from the earlier completion")
        .value;
    assert_eq!(
        (disabled.name.as_str(), disabled.enabled),
        ("pending rename", false)
    );
    f.assert_cache_is_committed();
}

#[test]
fn storage_cron_scheduler_port_draft_commits_after_lock_release() {
    let f = fixture();
    let config = codeterm_types::config::MemoryConfig::default();
    {
        let mut store = f.cron.lock();
        let port = &mut CronSchedulerPort(&mut store);
        assert!(
            ensure_dreaming_cron(port, &config, "claude", &CatalogBackgroundModelPort).unwrap()
        );
        assert!(
            store.list().is_empty(),
            "a draft is not published before it commits"
        );
    }
    assert!(f.snapshot().states.is_empty());
    assert_eq!(settle_port(&f.cron), PortSettlement::Committed);
    let job = f
        .cron
        .lock()
        .list()
        .iter()
        .find(|job| job.name == DREAMING_CRON_NAME)
        .cloned()
        .expect("dreaming cron");
    assert!(job.protected && job.enabled);

    let revision = f.storage.revision();
    {
        let mut store = f.cron.lock();
        let port = &mut CronSchedulerPort(&mut store);
        assert!(
            !ensure_dreaming_cron(port, &config, &job.agent, &CatalogBackgroundModelPort).unwrap()
        );
    }
    assert_eq!(
        settle_port(&f.cron),
        PortSettlement::Empty,
        "no edit opened a draft"
    );
    assert_eq!(
        f.storage.revision(),
        revision,
        "an unchanged draft writes nothing"
    );
    f.assert_cache_is_committed();
}

#[test]
fn storage_cron_fence_raised_after_prepare_refuses_admission() {
    let f = fixture();
    let renamed = f.create(params("unresolved"));
    let dispatched = f.create(params("fenced"));
    let before = f.storage.revision();
    let hold = held(&f.storage);
    let prepared = gate(&f.cron, CronStage::Prepared);
    let (cron, id) = (f.cron.clone(), dispatched.id.clone());
    let later = std::thread::spawn(move || record_fire_start(&cron, &id, "fenced-run"));
    prepared.wait_reached();

    f.cron.lock().set_storage_deadline(Duration::from_millis(1));
    let unresolved = commit(&f.cron, |store| {
        store.prepare_update(&renamed.id, rename("admitted rename"))
    });
    assert!(
        matches!(unresolved, Err(CronCommitError::Unconfirmed(_))),
        "{unresolved:?}"
    );
    f.cron.lock().set_storage_deadline(DEADLINE);
    prepared.release();
    assert!(
        !later.join().unwrap(),
        "a change prepared before the fence rose is never submitted or dispatched"
    );

    hold.release();
    f.storage
        .committed_after(before)
        .wait(DEADLINE)
        .expect("the admitted rename settles on its own");
    let settled = commit(&f.cron, |store| {
        store.prepare_set_enabled(&renamed.id, false)
    })
    .expect("the fence settles from the admitted completion")
    .value;
    assert_eq!(
        settled.name, "admitted rename",
        "published once, never replayed"
    );
    assert!(f.cron.lock().get_history(&dispatched.id).is_empty());
    let snapshot = f.snapshot();
    assert!(
        state_of(&snapshot, &dispatched.id).history.is_empty(),
        "the writer queue is FIFO, so a submitted run start would have committed by now"
    );
    f.assert_cache_is_committed();
}

fn open_dreaming_draft(f: &Fixture) -> bool {
    let config = codeterm_types::config::MemoryConfig::default();
    let mut store = f.cron.lock();
    let port = &mut CronSchedulerPort(&mut store);
    ensure_dreaming_cron(port, &config, "claude", &CatalogBackgroundModelPort).unwrap()
}

fn dreaming_rows(f: &Fixture) -> usize {
    let snapshot = f.snapshot();
    let dreaming = |state: &&crate::storage::CronJobState| {
        state
            .job
            .as_ref()
            .is_some_and(|job| job.record.name == DREAMING_CRON_NAME)
    };
    snapshot.states.iter().filter(dreaming).count()
}

#[test]
fn storage_cron_port_draft_is_retained_when_refused_and_never_replayed_when_uncertain() {
    let f = fixture_with(StorageConfig {
        mailbox_commands: 1,
        ..StorageConfig::default()
    });
    let filler = f.create(params("filler"));
    let hold = held(&f.storage);
    let submitted = gate(&f.cron, CronStage::Submitted);
    let (cron, id) = (f.cron.clone(), filler.id.clone());
    let queued = std::thread::spawn(move || record_fire_start(&cron, &id, "queued-run"));
    submitted.wait_reached();
    submitted.release();
    assert!(open_dreaming_draft(&f));
    let refused = settle_port(&f.cron);
    assert!(
        matches!(
            &refused,
            PortSettlement::Retained(CronCommitError::NotApplied(_))
        ),
        "{refused:?}"
    );
    {
        let mut store = f.cron.lock();
        let port = CronSchedulerPort(&mut store);
        assert!(
            port.find_job_by_name(DREAMING_CRON_NAME).is_some(),
            "the draft is kept"
        );
        assert!(store
            .list()
            .iter()
            .all(|job| job.name != DREAMING_CRON_NAME));
    }
    hold.release();
    assert!(queued.join().unwrap());
    assert_eq!(settle_port(&f.cron), PortSettlement::Committed);
    assert_eq!(dreaming_rows(&f), 1);

    let f = fixture();
    let before = f.storage.revision();
    assert!(open_dreaming_draft(&f));
    f.cron.lock().set_storage_deadline(Duration::from_millis(1));
    let hold = held(&f.storage);
    let uncertain = settle_port(&f.cron);
    assert!(
        matches!(uncertain, PortSettlement::Unconfirmed(_)),
        "{uncertain:?}"
    );
    assert_eq!(
        settle_port(&f.cron),
        PortSettlement::Empty,
        "nothing is replayed"
    );
    f.cron.lock().set_storage_deadline(DEADLINE);
    hold.release();
    f.storage
        .committed_after(before)
        .wait(DEADLINE)
        .expect("the admitted draft settles on its own");
    f.create(params("after the fence"));
    assert!(f
        .cron
        .lock()
        .list()
        .iter()
        .any(|job| job.name == DREAMING_CRON_NAME));
    assert_eq!(dreaming_rows(&f), 1, "committed exactly once");
    f.assert_cache_is_committed();
}
