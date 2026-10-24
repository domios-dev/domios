// The Blueprint JSON schema is one json! literal deeper than the default macro recursion limit.
#![recursion_limit = "256"]

pub mod automation_name;
pub mod blueprint_catalog;
pub mod blueprint_journal;
pub mod blueprint_pipeline;
pub mod blueprint_recipe;
pub mod blueprint_run;
pub mod blueprint_tags;
pub mod blueprint_terminal;
pub mod sentinel_input;

pub mod blueprint_github;

pub mod blueprint_document;
pub mod blueprint_execution;
pub mod blueprint_http;
pub mod blueprint_memory;
pub mod blueprint_source;

pub mod blueprint_watch;

pub mod blueprint_placeholder;

pub mod blueprint_contract;

pub mod blueprint_project;
pub mod code_source;
