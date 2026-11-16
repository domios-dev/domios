use crate::service::heartbeat_gate;
pub fn read(plan: &crate::plan::Plan) -> codeterm_plan::service_plan::ReminderAdmissionRead {
    codeterm_plan::service_plan::ReminderAdmissionRead::for_test(plan, heartbeat_gate::scrubbed)
}
pub fn reads(
    plans: &[crate::plan::Plan],
) -> Vec<codeterm_plan::service_plan::ReminderCandidateRead> {
    codeterm_plan::service_plan::ReminderCandidateRead::for_test(
        plans,
        None,
        heartbeat_gate::scrubbed,
    )
}
pub fn popup_reads(
    plans: &[crate::plan::Plan],
    tab: &str,
) -> Vec<codeterm_plan::service_plan::ReminderCandidateRead> {
    codeterm_plan::service_plan::ReminderCandidateRead::for_test(
        plans,
        Some(tab),
        heartbeat_gate::scrubbed,
    )
}
