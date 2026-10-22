use crate::store::{Owner, Sentinel, Store};
use codeterm_blueprint_types::blueprint_document::Document;
use serde_json::Value;

impl Store {
    pub fn prepare_document(
        &self,
        value: Value,
        owner: Owner,
        connected: bool,
    ) -> Result<Sentinel, String> {
        let mut document = Document::from_definition(&value)?;
        crate::seal_code::document(&mut document, std::path::Path::new(&owner.cwd))?;
        let value = document.bind(&[]).map_err(|errors| format!("{errors:?}"))?;
        let runtime = Document::from_definition(&value)?
            .lower()
            .map_err(|errors| format!("{errors:?}"))?;
        let mut job = if let Some(spec) = runtime.get("spec") {
            self.prepare_pipeline(
                serde_json::from_value(spec.clone()).map_err(|error| error.to_string())?,
                owner,
                connected,
            )?
        } else {
            self.prepare(
                serde_json::from_value(runtime["create"].clone())
                    .map_err(|error| error.to_string())?,
                owner,
                connected,
            )?
        };
        job.document = Some(value);
        Ok(job)
    }
    pub fn create_document(
        &mut self,
        value: Value,
        owner: Owner,
        connected: bool,
    ) -> Result<Sentinel, String> {
        let job = self.prepare_document(value, owner, connected)?;
        self.insert(job)
    }
}

pub fn literal_params(fields: Value) -> Value {
    Value::Object(
        fields
            .as_object()
            .expect("literal parameter mapping")
            .iter()
            .map(|(key, value)| (key.clone(), serde_json::json!({"lit":value})))
            .collect(),
    )
}

pub fn watch(
    name: Value,
    mut source: Value,
    mut decision: Value,
    effects: Value,
    note: Value,
) -> Value {
    source["id"] = serde_json::json!("source");
    decision["id"] = serde_json::json!("decide");
    decision["uses"] = serde_json::json!("ask");
    if source.get("timeout").is_none() {
        source["timeout"] = serde_json::json!({"lit":"7d"});
    }
    let effects = if effects.is_null() {
        Value::Object(decision["buckets"].as_array().expect("fixture buckets").iter().map(|bucket|{
            let bucket=bucket.as_str().expect("fixture bucket");(bucket.into(),serde_json::json!([{"id":crate::handlers::Handler::notify(bucket).id,"uses":"notify"}]))
        }).collect())
    } else {
        effects
    };
    let mut nodes = vec![source, decision];
    let mut routes = serde_json::Map::new();
    for (bucket, chain) in effects.as_object().expect("fixture effect chains") {
        let chain = chain.as_array().expect("fixture native effect nodes");
        routes.insert(
            bucket.clone(),
            chain
                .first()
                .map_or(serde_json::json!("@stop"), |node| node["id"].clone()),
        );
        for (index, node) in chain.iter().enumerate() {
            let mut node = node.clone();
            node["routes"] = serde_json::json!({"done":chain.get(index+1).map_or(serde_json::json!("@stop"),|node|node["id"].clone())});
            nodes.push(node);
        }
    }
    nodes[1]["routes"] = Value::Object(routes);
    serde_json::json!({"schema_version":2,"kind":"watch","name":name,"note":note,"steps":nodes})
}
