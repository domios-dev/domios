use std::collections::VecDeque;

use codeterm_plugins::update_source::github_release::{ApiBudgetStore, RateHeaders};

use super::{Body, Get, Http, Reply, Route, Transport};

pub(crate) const NOW_MS: u64 = 1_800_000_000_000;

pub(crate) enum Step {
    Status(u16),
    Bytes(Vec<u8>),
    Fails(&'static str),
    BreaksAfter(Vec<u8>, &'static str),
    RateLimited { status: u16, reset_unix: u64 },
}

struct ScriptedBody(VecDeque<Result<Vec<u8>, String>>);

#[async_trait::async_trait]
impl Body for ScriptedBody {
    async fn chunk(&mut self) -> Result<Option<Vec<u8>>, String> {
        self.0.pop_front().transpose()
    }
}

fn reply(status: u16, rate: RateHeaders, chunks: Vec<Result<Vec<u8>, String>>) -> Reply {
    Reply {
        status,
        etag: Some("\"validator\"".into()),
        retry_after: None,
        rate,
        body: Box::new(ScriptedBody(chunks.into())),
    }
}

pub(crate) struct Seen {
    pub url: String,
    pub route: Route,
    pub accept: Option<String>,
}

#[derive(Default)]
pub(crate) struct ScriptedHttp {
    steps: parking_lot::Mutex<VecDeque<(&'static str, Step)>>,
    seen: parking_lot::Mutex<Vec<Seen>>,
}

impl ScriptedHttp {
    pub fn new(steps: Vec<(&'static str, Step)>) -> Self {
        Self {
            steps: parking_lot::Mutex::new(steps.into()),
            seen: parking_lot::Mutex::default(),
        }
    }

    pub fn urls(&self) -> Vec<String> {
        self.seen
            .lock()
            .iter()
            .map(|seen| seen.url.clone())
            .collect()
    }

    pub fn seen(&self) -> parking_lot::MutexGuard<'_, Vec<Seen>> {
        self.seen.lock()
    }
}

#[async_trait::async_trait]
impl Http for ScriptedHttp {
    async fn get(&self, request: Get<'_>) -> Result<Reply, String> {
        self.seen.lock().push(Seen {
            url: request.url.into(),
            route: request.route,
            accept: request.accept.map(str::to_owned),
        });
        let (expected, step) = self
            .steps
            .lock()
            .pop_front()
            .unwrap_or_else(|| panic!("unexpected request to {}", request.url));
        assert_eq!(request.url, expected);
        match step {
            Step::Status(status) => Ok(reply(status, RateHeaders::default(), Vec::new())),
            Step::Bytes(bytes) => Ok(reply(200, RateHeaders::default(), vec![Ok(bytes)])),
            Step::Fails(cause) => Err(cause.into()),
            Step::BreaksAfter(bytes, cause) => Ok(reply(
                200,
                RateHeaders::default(),
                vec![Ok(bytes), Err(cause.into())],
            )),
            Step::RateLimited { status, reset_unix } => Ok(reply(
                status,
                RateHeaders {
                    remaining: Some(0),
                    reset_unix: Some(reset_unix),
                    retry_after_secs: None,
                },
                Vec::new(),
            )),
        }
    }
}

pub(crate) fn transport<'a>(http: &'a ScriptedHttp, root: &tempfile::TempDir) -> Transport<'a> {
    Transport {
        http,
        budget: ApiBudgetStore::at(root.path().join("budget.json")),
        now: Box::new(|| NOW_MS),
    }
}
