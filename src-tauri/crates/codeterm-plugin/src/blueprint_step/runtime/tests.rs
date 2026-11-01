use super::*;

#[test]
fn plugin_native_late_or_cancelled_update_retains_job_as_unknown() {
    let request:StepRequest = serde_json::from_value(serde_json::json!({"context":{"scope":{"workspace":"workspace","team":null,"run":"run"},"owner":"owner","node":"node","attempt":"attempt","operation":"operation","nodeLabel":"Node","inputLineage":[]},"nodeType":"fixture.step","revision":1,"params":{},"inputs":{}})).unwrap();
    for expired in [false, true] {
        let cancellation = ConversionCancellation::default();
        if !expired {
            cancellation.cancel();
        }
        let call = StepCall {
            request: &request,
            job: None,
            deadline: if expired {
                Instant::now()
            } else {
                Instant::now() + std::time::Duration::from_secs(2)
            },
            cancellation,
        };
        let update = bounded_update(
            Ok(JobUpdate {
                job: "fixture-job".into(),
                state: OperationState::Succeeded,
                outcome: Some("done".into()),
                outputs: PortValues::new(),
            }),
            &call,
        )
        .unwrap();
        assert_eq!(update, JobUpdate::unknown("fixture-job".into()));
    }
}
