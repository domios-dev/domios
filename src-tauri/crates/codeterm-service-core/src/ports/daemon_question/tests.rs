#[test]
fn daemon_question_port_is_thread_safe() {
    super::super::assert_port::<dyn super::DaemonQuestionHandler>();
}
