use codeterm_types::pty::BootStage;

#[test]
fn boot_stage_serde_round_trip_new_variants() {
    for stage in [
        BootStage::PreparingRoleInstructions,
        BootStage::DeliveringInstructions,
    ] {
        let encoded = serde_json::to_string(&stage).unwrap();
        assert_eq!(serde_json::from_str::<BootStage>(&encoded).unwrap(), stage);
    }
    assert_eq!(
        serde_json::to_string(&BootStage::PreparingRoleInstructions).unwrap(),
        "\"preparing_role_instructions\""
    );
    assert_eq!(
        serde_json::to_string(&BootStage::DeliveringInstructions).unwrap(),
        "\"delivering_instructions\""
    );
}
