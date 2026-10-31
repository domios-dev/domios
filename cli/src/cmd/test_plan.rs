pub const TEST_PLAN_VAGUE_BANNER: &str =
    "VERIFICATION PLAN IS MISSING — name the surface, action and observable pass condition.";

pub fn test_plan_vague_reason(test_plan: &str) -> Option<String> {
    test_plan
        .trim()
        .is_empty()
        .then(|| "verification plan is empty".to_owned())
}

pub fn test_plan_is_vague(test_plan: &str) -> bool {
    test_plan_vague_reason(test_plan).is_some()
}
