#[test]
fn exact_first_user_ack_and_final_answer_persist_a_real_worker_report() {
    crate::register_orchestration();
    codeterm_service_core::service::reply_relay::worker::integration::exact_first_user_ack_and_final_answer_persist_a_real_worker_report(
        codeterm_service_core::service::test_fixture::ServiceFixture::new,
    );
}
