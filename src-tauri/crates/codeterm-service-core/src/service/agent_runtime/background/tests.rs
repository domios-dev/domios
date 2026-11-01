use super::*;
use std::collections::VecDeque;

struct Fake {
    results: VecDeque<Result<RuntimeResult, RuntimeFailure>>,
    calls: Vec<RuntimeOperation>,
    verified: bool,
    finished: bool,
    os_absent: bool,
    unknown_present: bool,
    notices: Vec<String>,
    exited: bool,
    observed_tree_exit: bool,
}
#[async_trait::async_trait]
impl CleanupIo for Fake {
    fn ended(&self) -> bool {
        self.observed_tree_exit
    }
    async fn run(&mut self, op: RuntimeOperation) -> Result<RuntimeResult, RuntimeFailure> {
        self.calls.push(op);
        self.results.pop_front().expect("unexpected cleanup call")
    }
    async fn verify(
        &mut self,
        roster: Vec<RuntimeBackgroundProcess>,
    ) -> Result<Vec<u32>, RuntimeFailure> {
        self.verified = true;
        let _ = roster;
        if self.os_absent {
            Ok(if self.unknown_present {
                vec![42]
            } else {
                Vec::new()
            })
        } else {
            Err(refused(
                "roster-matched OS process survived; server retained",
            ))
        }
    }
    fn notice(&mut self, unknown: &[u32]) {
        self.notices.push(format!("retained {unknown:?}"));
    }
    async fn finish(&mut self) -> Result<(), RuntimeFailure> {
        self.finished = true;
        if self.exited {
            Ok(())
        } else {
            Err(RuntimeFailure::Deadline)
        }
    }
}
fn process() -> RuntimeBackgroundProcess {
    RuntimeBackgroundProcess {
        process_id: "p".into(),
        item_id: "i".into(),
        command: "opaque".into(),
        cwd: "/work".into(),
        os_pid: None,
    }
}
fn page_result(
    items: Vec<RuntimeBackgroundProcess>,
    cursor: Option<&str>,
) -> Result<RuntimeResult, RuntimeFailure> {
    Ok(RuntimeResult::BackgroundProcesses {
        thread_id: "root".into(),
        processes: items,
        next_cursor: cursor.map(str::to_string),
    })
}
fn context() -> RuntimeContext {
    RuntimeContext {
        restricted_cli: None,
        thread_id: Some("root".into()),
        ..Default::default()
    }
}
fn fake(residual: bool) -> Fake {
    Fake {
        results: VecDeque::from([
            page_result(vec![process()], Some("next")),
            page_result(Vec::new(), None),
            Ok(RuntimeResult::BackgroundProcessTermination {
                thread_id: "root".into(),
                process_id: "p".into(),
                acknowledged: true,
            }),
            page_result(
                if residual {
                    vec![process()]
                } else {
                    Vec::new()
                },
                None,
            ),
            Ok(RuntimeResult::Done),
        ]),
        calls: Vec::new(),
        verified: false,
        finished: false,
        os_absent: true,
        unknown_present: false,
        notices: Vec::new(),
        exited: true,
        observed_tree_exit: false,
    }
}

#[tokio::test]
async fn interrupt_consumes_pages_then_terminates_lists_cleans_and_verifies_os_without_stopping_server(
) {
    let mut io = fake(false);
    coordinate(&mut io, &context(), Mode::Interrupt, true)
        .await
        .unwrap();
    assert!(io.verified);
    assert!(!io.finished);
    assert!(matches!(
        &io.calls[0],
        RuntimeOperation::BackgroundProcesses {
            action: RuntimeBackgroundProcessAction::List { cursor: None },
            ..
        }
    ));
    assert!(
        matches!(&io.calls[1],RuntimeOperation::BackgroundProcesses{action:RuntimeBackgroundProcessAction::List{cursor:Some(next)},..} if next=="next")
    );
    assert!(matches!(
        &io.calls[2],
        RuntimeOperation::BackgroundProcesses {
            action: RuntimeBackgroundProcessAction::Terminate { .. },
            ..
        }
    ));
    assert!(matches!(
        &io.calls[3],
        RuntimeOperation::BackgroundProcesses {
            action: RuntimeBackgroundProcessAction::List { cursor: None },
            ..
        }
    ));
    assert!(matches!(
        &io.calls[4],
        RuntimeOperation::BackgroundProcesses {
            action: RuntimeBackgroundProcessAction::Clean,
            ..
        }
    ));
}
#[tokio::test]
async fn acknowledgement_is_not_absence_and_roster_survivors_fail() {
    let mut io = fake(true);
    assert!(coordinate(&mut io, &context(), Mode::Interrupt, true)
        .await
        .is_err());
    assert!(io.verified);
    assert!(!io.finished);
    let mut io = fake(false);
    io.os_absent = false;
    assert!(coordinate(&mut io, &context(), Mode::Interrupt, true)
        .await
        .unwrap_err()
        .to_string()
        .contains("server retained"));
    assert!(!io.finished);
}
#[tokio::test]
async fn stop_close_and_failure_require_observed_tree_exit_even_when_rpc_is_broken() {
    for mode in ["stop", "close", "failure"] {
        let mut io = fake(false);
        io.results = VecDeque::from([Err(RuntimeFailure::PipeEof)]);
        coordinate(&mut io, &context(), Mode::Shutdown, true)
            .await
            .unwrap();
        assert!(io.finished, "{mode}");
        assert!(!io.verified);
        let mut io = fake(false);
        io.exited = false;
        assert!(coordinate(&mut io, &context(), Mode::Shutdown, false)
            .await
            .is_err());
        assert!(io.finished);
    }
}
#[tokio::test]
async fn repeated_cursors_never_prove_absence() {
    let mut io = fake(false);
    io.results = VecDeque::from([
        page_result(vec![process()], Some("same")),
        page_result(Vec::new(), Some("same")),
    ]);
    assert!(coordinate(&mut io, &context(), Mode::Interrupt, true)
        .await
        .is_err());
    assert!(io.verified);
    assert!(!io.finished);
}
#[tokio::test]
async fn owned_descendants_are_listed_even_when_idle_and_foreign_threads_are_excluded() {
    let mut ctx = context();
    ctx.thread_parents.push(RuntimeThreadParent {
        thread_id: "child".into(),
        parent_thread_id: "root".into(),
    });
    let mut io = fake(false);
    io.results.extend([
        Ok(RuntimeResult::BackgroundProcesses {
            thread_id: "child".into(),
            processes: Vec::new(),
            next_cursor: None,
        }),
        Ok(RuntimeResult::BackgroundProcesses {
            thread_id: "child".into(),
            processes: Vec::new(),
            next_cursor: None,
        }),
        Ok(RuntimeResult::Done),
    ]);
    coordinate(&mut io, &ctx, Mode::Interrupt, true)
        .await
        .unwrap();
    assert_eq!(io.calls.iter().filter(|op|matches!(op,RuntimeOperation::BackgroundProcesses{thread_id,..} if thread_id=="child")).count(),3);
    assert!(!io.finished);
}

#[tokio::test]
async fn empty_provider_roster_still_requires_os_absence() {
    let mut io = fake(false);
    io.os_absent = false;
    io.results = VecDeque::from([
        page_result(Vec::new(), None),
        page_result(Vec::new(), None),
        Ok(RuntimeResult::Done),
    ]);
    assert!(coordinate(&mut io, &context(), Mode::Interrupt, true)
        .await
        .is_err());
    assert!(io.verified);
    assert!(!io.finished);
}
#[tokio::test]
async fn verified_tree_exit_counts_as_termination_but_pipe_eof_alone_does_not() {
    let mut io = fake(false);
    io.observed_tree_exit = true;
    io.results = VecDeque::from([Err(RuntimeFailure::PipeEof)]);
    coordinate(&mut io, &context(), Mode::Interrupt, true)
        .await
        .unwrap();
    assert!(!io.finished);
    assert!(!io.verified);
    let mut io = fake(false);
    io.os_absent = false;
    io.results = VecDeque::from([Err(RuntimeFailure::PipeEof)]);
    assert!(coordinate(&mut io, &context(), Mode::Interrupt, true)
        .await
        .is_err());
    assert!(!io.finished);
    let mut io = fake(false);
    io.exited = false;
    io.results = VecDeque::from([Err(RuntimeFailure::PipeEof)]);
    assert!(coordinate(&mut io, &context(), Mode::Shutdown, true)
        .await
        .is_err());
}
#[tokio::test]
async fn full_shutdown_interrupts_owned_turns_before_listing() {
    let mut ctx = context();
    ctx.active_turn_id = Some("turn".into());
    let mut io = fake(false);
    io.results.push_front(Ok(RuntimeResult::Done));
    coordinate(&mut io, &ctx, Mode::Shutdown, true)
        .await
        .unwrap();
    assert!(
        matches!(&io.calls[0],RuntimeOperation::Interrupt{thread_id,active_turn_id:Some(turn)} if thread_id=="root" && turn=="turn")
    );
    assert!(io.finished);
}

#[tokio::test]
async fn unknown_members_are_retained_with_notice_and_interrupt_succeeds() {
    let mut io = fake(false);
    io.unknown_present = true;
    coordinate(&mut io, &context(), Mode::Interrupt, true)
        .await
        .unwrap();
    assert_eq!(io.notices, ["retained [42]"]);
    assert!(!io.finished);
    assert!(io.verified);
}
