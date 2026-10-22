use serde_json::Value;
pub fn schema() -> Value {
    serde_json::to_value(schemars::schema_for!(super::Document))
        .expect("generated schema is serializable")
}
