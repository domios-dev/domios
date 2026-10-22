use std::sync::{mpsc, Arc};
use std::time::Duration;

use super::key::{ApiKey, CachedKey};
use super::provider::{self, Content, EmbedError, EmbeddingProvider, Modality};

pub const ID: provider::Id = "openai";
pub const DEFAULT_ENDPOINT: &str = "https://api.openai.com";
pub const BATCH_SIZE: usize = 64;

const MODALITIES: &[Modality] = &[Modality::Text];
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, PartialEq, Eq)]
pub struct OpenAiModel {
    pub model: &'static str,
    pub native_dimension: usize,
    pub max_tokens: usize,
}

pub static MODELS: &[OpenAiModel] = &[
    OpenAiModel {
        model: "text-embedding-3-small",
        native_dimension: 1536,
        max_tokens: 8191,
    },
    OpenAiModel {
        model: "text-embedding-3-large",
        native_dimension: 3072,
        max_tokens: 8191,
    },
];

pub fn find(model: &str) -> Option<&'static OpenAiModel> {
    MODELS.iter().find(|row| row.model == model)
}

pub struct OpenAiProvider {
    model: &'static OpenAiModel,
    dimension: usize,
    url: String,
    key: Arc<CachedKey>,
    http: HttpWorker,
}

impl OpenAiProvider {
    pub fn new(
        model: &'static OpenAiModel,
        dimension: usize,
        endpoint: &str,
        key: Arc<CachedKey>,
    ) -> Result<Self, String> {
        #[cfg(test)]
        {
            let url = reqwest::Url::parse(endpoint).map_err(|error| error.to_string())?;
            if url.scheme() != "http"
                || !url.host_str().is_some_and(|host| {
                    host.parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
                })
            {
                return Err("unit tests require a loopback mock endpoint".into());
            }
        }
        if dimension == 0 || dimension > model.native_dimension {
            return Err(format!(
                "{} supports 1..={} dimensions, got {dimension}",
                model.model, model.native_dimension
            ));
        }
        Ok(Self {
            model,
            dimension,
            url: format!("{}/v1/embeddings", endpoint.trim_end_matches('/')),
            key,
            http: HttpWorker::start()?,
        })
    }

    fn embed_texts(&self, texts: &[&str]) -> Vec<Result<Vec<f32>, EmbedError>> {
        let mut out = Vec::with_capacity(texts.len());
        for batch in texts.chunks(BATCH_SIZE) {
            match self.request(batch) {
                Ok(vectors) => out.extend(vectors.into_iter().map(Ok)),
                Err(error) => out.extend(batch.iter().map(|_| Err(error.clone()))),
            }
        }
        out
    }

    fn request(&self, batch: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        let key = self.key.get().map_err(EmbedError::Unavailable)?;
        let input: Vec<String> = batch
            .iter()
            .map(|text| fit_to_tokens(text, self.model.max_tokens))
            .collect();
        let body = serde_json::json!({
            "model": self.model.model,
            "input": input,
            "dimensions": self.dimension,
            "encoding_format": "float",
        });
        let (status, text) = self
            .http
            .post(&self.url, key.clone(), body)
            .map_err(|error| EmbedError::Unavailable(format!("network: {error}")))?;
        if status == 401 || status == 403 {
            self.key.auth_failed(&key);
        }
        if status != 200 {
            return Err(map_status(status, &text));
        }
        parse_vectors(&text, batch.len(), self.dimension)
    }
}

pub fn map_status(status: u16, body: &str) -> EmbedError {
    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value["error"]["message"].as_str().map(str::to_owned))
        .unwrap_or_default();
    match status {
        401 | 403 => EmbedError::Unavailable(format!("authentication failed ({status})")),
        429 => EmbedError::Unavailable("rate limited (429)".into()),
        500..=599 => EmbedError::Unavailable(format!("server error ({status})")),
        _ => EmbedError::Refused(format!("request rejected ({status}): {detail}")),
    }
}

fn parse_vectors(
    body: &str,
    expected: usize,
    dimension: usize,
) -> Result<Vec<Vec<f32>>, EmbedError> {
    #[derive(serde::Deserialize)]
    struct Datum {
        index: usize,
        embedding: Vec<f32>,
    }
    #[derive(serde::Deserialize)]
    struct Response {
        data: Vec<Datum>,
    }
    let mut response: Response = serde_json::from_str(body)
        .map_err(|e| EmbedError::Unavailable(format!("malformed response: {e}")))?;
    response.data.sort_by_key(|datum| datum.index);
    let indexes_match = response.data.len() == expected
        && response
            .data
            .iter()
            .enumerate()
            .all(|(position, datum)| datum.index == position);
    if !indexes_match {
        return Err(EmbedError::Unavailable(format!(
            "response has {} vectors for {expected} inputs",
            response.data.len()
        )));
    }
    response
        .data
        .into_iter()
        .map(|datum| {
            provider::validate_vector(&datum.embedding, dimension)?;
            Ok(datum.embedding)
        })
        .collect()
}

/// Conservative token estimate: four ASCII bytes or one other char per token.
pub fn estimate_tokens(text: &str) -> usize {
    let ascii = text.bytes().filter(u8::is_ascii).count();
    let other = text.chars().filter(|c| !c.is_ascii()).count();
    ascii.div_ceil(4) + other
}

pub fn fit_to_tokens(text: &str, max_tokens: usize) -> String {
    if estimate_tokens(text) <= max_tokens {
        return text.to_string();
    }
    let (mut ascii, mut other, mut end) = (0usize, 0usize, 0usize);
    for (offset, c) in text.char_indices() {
        if c.is_ascii() {
            ascii += 1;
        } else {
            other += 1;
        }
        if ascii.div_ceil(4) + other > max_tokens {
            break;
        }
        end = offset + c.len_utf8();
    }
    text[..end].to_string()
}

impl EmbeddingProvider for OpenAiProvider {
    fn id(&self) -> &str {
        ID
    }

    fn model(&self) -> &str {
        self.model.model
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn max_tokens(&self) -> usize {
        self.model.max_tokens
    }

    fn modalities(&self) -> &[Modality] {
        MODALITIES
    }

    fn remote(&self) -> bool {
        true
    }

    fn embed_passage(&self, content: &Content) -> Result<Vec<f32>, EmbedError> {
        let text = provider::supported_text(MODALITIES, content)?;
        self.embed_texts(&[text]).remove(0)
    }

    fn embed_query(&self, content: &Content) -> Result<Vec<f32>, EmbedError> {
        self.embed_passage(content)
    }

    fn embed_passages(&self, contents: &[Content]) -> Vec<Result<Vec<f32>, EmbedError>> {
        let mut results: Vec<Option<Result<Vec<f32>, EmbedError>>> = contents
            .iter()
            .map(|content| provider::supported_text(MODALITIES, content).err().map(Err))
            .collect();
        let texts: Vec<&str> = contents
            .iter()
            .filter_map(|content| provider::supported_text(MODALITIES, content).ok())
            .collect();
        let mut embedded = self.embed_texts(&texts).into_iter();
        for slot in results.iter_mut().filter(|slot| slot.is_none()) {
            *slot = embedded.next();
        }
        results.into_iter().flatten().collect()
    }
}

type HttpReply = Result<(u16, String), String>;

struct HttpJob {
    url: String,
    key: ApiKey,
    body: serde_json::Value,
    reply: mpsc::SyncSender<HttpReply>,
}

/// The blocking HTTP client lives on its own thread so no caller, async
/// runtime worker included, ever drives it directly.
struct HttpWorker {
    jobs: mpsc::Sender<HttpJob>,
}

impl HttpWorker {
    fn start() -> Result<Self, String> {
        let (jobs, inbox) = mpsc::channel::<HttpJob>();
        std::thread::Builder::new()
            .name("mem-embed-openai".into())
            .spawn(move || {
                let builder = reqwest::blocking::Client::builder()
                    .connect_timeout(CONNECT_TIMEOUT)
                    .timeout(REQUEST_TIMEOUT);
                #[cfg(test)]
                let builder = builder
                    .no_proxy()
                    .redirect(reqwest::redirect::Policy::none());
                let client = builder.build();
                for job in inbox {
                    let reply = match &client {
                        Ok(client) => send(client, &job),
                        Err(error) => Err(format!("build HTTP client: {error}")),
                    };
                    let _ = job.reply.send(reply);
                }
            })
            .map_err(|e| format!("start OpenAI embedder worker: {e}"))?;
        Ok(Self { jobs })
    }

    fn post(&self, url: &str, key: ApiKey, body: serde_json::Value) -> HttpReply {
        let (reply, response) = mpsc::sync_channel(1);
        self.jobs
            .send(HttpJob {
                url: url.to_string(),
                key,
                body,
                reply,
            })
            .map_err(|_| "OpenAI embedder worker stopped".to_string())?;
        response
            .recv()
            .map_err(|_| "OpenAI embedder worker stopped".to_string())?
    }
}

fn send(client: &reqwest::blocking::Client, job: &HttpJob) -> HttpReply {
    let response = client
        .post(&job.url)
        .bearer_auth(job.key.expose())
        .json(&job.body)
        .send()
        .map_err(|e| e.without_url().to_string())?;
    let status = response.status().as_u16();
    let text = response.text().map_err(|e| e.without_url().to_string())?;
    Ok((status, text))
}
