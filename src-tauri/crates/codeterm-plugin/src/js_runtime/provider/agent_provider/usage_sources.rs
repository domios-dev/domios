use super::session_wire::WireUsageSource;
use super::{provider, JsProvider, UsageSourceSpec};
use serde_json::json;
use std::path::PathBuf;

impl JsProvider {
    pub(super) fn js_usage_accounting(&self) -> crate::token_usage::UsageAccounting {
        if self.has_js("usageAccounting") {
            return self.try_call("usageAccounting", &[]).unwrap_or_default();
        }
        self.native()
            .as_ref()
            .map(|p| p.usage_accounting())
            .unwrap_or_default()
    }

    pub(super) fn js_parse_usage_record(
        &self,
        record: &serde_json::Value,
        now: u64,
    ) -> Result<Option<crate::token_usage::ParsedUsage>, String> {
        if self.has_js("parseUsageRecord") {
            return super::super::call_json(
                &self.vm,
                "parseUsageRecord",
                &[record.clone(), json!(now)],
            )
            .map_err(|e| e.to_string());
        }
        match self.native() {
            Some(provider) => provider.parse_usage_record(record, now),
            None => Ok(None),
        }
    }

    pub(super) fn js_usage_sources(&self, cwd: &str, session_id: &str) -> Vec<UsageSourceSpec> {
        if self.has_js("usageSources") {
            // An exported `usageSources` is authoritative: a null or failed reply
            // declares no source and must not fall back to a fabricated file.
            return self
                .try_call::<Vec<WireUsageSource>>("usageSources", &[json!(cwd), json!(session_id)])
                .unwrap_or_default()
                .into_iter()
                .filter(|source| !source.path.is_empty())
                .map(|source| UsageSourceSpec {
                    path: PathBuf::from(source.path),
                    session_id: source.session_id.filter(|id| !id.is_empty()),
                })
                .collect();
        }
        provider::usage_sources_from_session_file(self, cwd, session_id)
    }
}

macro_rules! js_usage_contract {
    () => {
        fn usage_sources(&self, cwd: &str, session_id: &str) -> Vec<UsageSourceSpec> {
            self.js_usage_sources(cwd, session_id)
        }

        fn usage_sql_query(&self, scoped: bool) -> Option<String> {
            if self.has_js("usageSqlQuery") {
                return self.try_call("usageSqlQuery", &[json!(scoped)]).flatten();
            }
            self.native().and_then(|p| p.usage_sql_query(scoped))
        }

        fn usage_accounting(&self) -> crate::token_usage::UsageAccounting {
            self.js_usage_accounting()
        }

        fn parse_usage_record(
            &self,
            record: &serde_json::Value,
            now: u64,
        ) -> Result<Option<crate::token_usage::ParsedUsage>, String> {
            self.js_parse_usage_record(record, now)
        }
    };
}
