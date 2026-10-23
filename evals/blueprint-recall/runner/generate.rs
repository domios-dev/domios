use crate::fixtures::{self, CachedEnrichment, CachedIntents, SaveInputs};
use crate::library::{self, Intents, WORKER_MODEL};
use codeterm_blueprint_types::blueprint_memory::validate_intents;
use codeterm_mem::blueprint_library::enrichment::parse_enrichment_report;
use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;

pub const INTENT_MODEL: &str = "opus";
pub const INTENT_PROMPT: &str = "You built the Blueprint below, ran it, and it worked. Save it to your Team's library with `domios blueprint keep <id> --when ...`. The library docs say: give 3 to 5 --when intents, each a different way someone would ask for this Blueprint; each intent is searched on its own and the best match counts, so vary the wording rather than repeating one phrase.\n\nWrite those 3 to 5 intents. Each is one short request of at most 20 words, in plain words a teammate would type when they need this. Output only a JSON array of strings, nothing else.\n\nBlueprint file:\n```yaml\n{file}\n```\n";

pub fn claude() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(".local/bin/claude")
}

pub fn command_line(model: &str) -> Vec<String> {
    let mut args = vec!["-p".to_owned(), "--model".into(), model.into()];
    if model == INTENT_MODEL {
        args.extend(["--effort".into(), "high".into()]);
    }
    args.extend([
        "--output-format".into(),
        "json".into(),
        "--tools".into(),
        String::new(),
        "--strict-mcp-config".into(),
    ]);
    args
}

pub fn run_claude(model: &str, prompt: &str) -> Result<Value, String> {
    let cwd = tempfile::tempdir().map_err(|error| error.to_string())?;
    let mut command = Command::new(claude());
    command
        .args(command_line(model))
        .current_dir(cwd.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("CODETERM_") {
            command.env_remove(key);
        }
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("spawn claude: {error}"))?;
    child
        .stdin
        .take()
        .ok_or("claude stdin")?
        .write_all(prompt.as_bytes())
        .map_err(|error| error.to_string())?;
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "claude exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|error| format!("claude json: {error}"))
}

enum Message<R> {
    Done(R),
    Paused(String),
}

pub fn parallel<T: Send + 'static, R: Send + 'static>(
    jobs: Vec<T>,
    workers: usize,
    work: fn(T) -> R,
    mut each: impl FnMut(R) -> Result<(), String>,
) -> Result<Option<String>, String> {
    let (job_tx, job_rx) = mpsc::channel::<T>();
    let job_rx = std::sync::Arc::new(parking_lot::Mutex::new(job_rx));
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (result_tx, result_rx) = mpsc::channel::<Message<R>>();
    for job in jobs {
        let _ = job_tx.send(job);
    }
    drop(job_tx);
    let handles = (0..workers.max(1))
        .map(|_| {
            let job_rx = job_rx.clone();
            let result_tx = result_tx.clone();
            let stop = stop.clone();
            std::thread::spawn(move || loop {
                if stop.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }
                let next = job_rx.lock().recv();
                let Ok(job) = next else { break };
                if let Err(paused) = crate::quota::gate() {
                    stop.store(true, std::sync::atomic::Ordering::SeqCst);
                    let _ = result_tx.send(Message::Paused(paused.reason));
                    break;
                }
                if result_tx.send(Message::Done(work(job))).is_err() {
                    break;
                }
            })
        })
        .collect::<Vec<_>>();
    drop(result_tx);
    let mut paused = None;
    for message in result_rx.iter() {
        match message {
            Message::Done(result) => each(result)?,
            Message::Paused(reason) => {
                paused.get_or_insert(reason);
            }
        }
    }
    for handle in handles {
        let _ = handle.join();
    }
    Ok(paused)
}

fn record_pause(reason: &str, pending: usize) -> Result<(), String> {
    let path = fixtures::eval_dir().join("save-inputs/pauses.jsonl");
    let line = serde_json::json!({"at": chrono::Utc::now().to_rfc3339(), "reason": reason, "not_launched": pending}).to_string();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    writeln!(file, "{line}").map_err(|error| error.to_string())
}

struct IntentJob {
    doc: String,
    hash: String,
    prompt: String,
}

type IntentResult = (String, String, Result<(Vec<String>, Value), String>);

fn intent_session(job: IntentJob) -> IntentResult {
    let outcome = run_claude(INTENT_MODEL, &job.prompt).and_then(|raw| {
        let text = raw["result"].as_str().ok_or("claude result missing")?;
        let start = text.find('[').ok_or("no JSON array in result")?;
        let end = text.rfind(']').ok_or("no JSON array in result")?;
        let intents: Vec<String> =
            serde_json::from_str(&text[start..=end]).map_err(|error| error.to_string())?;
        Ok((validate_intents(&intents)?, raw))
    });
    (job.doc, job.hash, outcome)
}

struct EnrichJob {
    doc: String,
    hash: String,
    task: String,
}

type EnrichResult = (String, String, Result<Value, String>);

fn enrich_session(job: EnrichJob) -> EnrichResult {
    (job.doc, job.hash, run_claude(WORKER_MODEL, &job.task))
}

fn session_file(doc: &str, kind: &str) -> (PathBuf, String) {
    let relative = format!("save-inputs/sessions/{doc}.{kind}.json");
    (fixtures::eval_dir().join(&relative), relative)
}

pub fn generate(workers: usize) -> Result<(), String> {
    let corpus = fixtures::corpus()?;
    let library = library::ingest(&corpus, Intents::Placeholder, None)?;
    let mut intents: SaveInputs<CachedIntents> =
        fixtures::read_json(&fixtures::intents_path()).unwrap_or_else(|_| SaveInputs {
            about: "Simulated save-time agent intents: one fresh headless session per pipeline that sees only that pipeline file; cached by document hash.".into(),
            model: INTENT_MODEL.into(),
            command: command_line(INTENT_MODEL),
            prompt: INTENT_PROMPT.into(),
            docs: Default::default(),
        });
    let mut enrichment: SaveInputs<CachedEnrichment> =
        fixtures::read_json(&fixtures::enrichment_path()).unwrap_or_else(|_| SaveInputs {
            about: "Simulated hidden mem worker enrichment: the product task (enrichment_task over the stored YAML and step gists) run once per pipeline; cached by document hash.".into(),
            model: WORKER_MODEL.into(),
            command: command_line(WORKER_MODEL),
            prompt: "codeterm_mem::prompt_text::BLUEPRINT_ENRICHMENT".into(),
            docs: Default::default(),
        });
    eprintln!("refused by the product save path: {:?}", library.refused);
    let intent_jobs = corpus
        .iter()
        .filter(|doc| library.jobs.contains_key(&doc.id))
        .filter_map(|doc| {
            let hash = library.jobs[&doc.id].document_hash.clone();
            (intents.docs.get(&doc.id).map(|c| &c.document_hash) != Some(&hash)).then(|| {
                IntentJob {
                    doc: doc.id.clone(),
                    hash,
                    prompt: INTENT_PROMPT.replace("{file}", doc.source.trim_end()),
                }
            })
        })
        .collect::<Vec<_>>();
    let enrich_jobs = library
        .jobs
        .iter()
        .filter(|(doc, job)| {
            enrichment.docs.get(*doc).map(|c| &c.document_hash) != Some(&job.document_hash)
        })
        .map(|(doc, job)| EnrichJob {
            doc: doc.clone(),
            hash: job.document_hash.clone(),
            task: job.task.clone(),
        })
        .collect::<Vec<_>>();
    eprintln!(
        "sessions: {} intent, {} enrichment",
        intent_jobs.len(),
        enrich_jobs.len()
    );
    let mut failures = Vec::new();
    let total_intents = intent_jobs.len();
    let mut done_intents = 0;
    let paused_intents = parallel(
        intent_jobs,
        workers,
        intent_session,
        |(doc, hash, outcome)| {
            done_intents += 1;
            match outcome {
                Ok((list, raw)) => {
                    let (path, relative) = session_file(&doc, "intents");
                    fixtures::write_json(&path, &raw)?;
                    intents.docs.insert(
                        doc,
                        CachedIntents {
                            document_hash: hash,
                            intents: list,
                            session: relative,
                        },
                    );
                    fixtures::write_json(&fixtures::intents_path(), &intents)
                }
                Err(error) => {
                    failures.push(format!("{doc} intents: {error}"));
                    Ok(())
                }
            }
        },
    )?;
    fixtures::write_json(&fixtures::intents_path(), &intents)?;
    if let Some(reason) = &paused_intents {
        record_pause(reason, total_intents - done_intents)?;
        return Err(format!(
            "paused before {} intent sessions: {reason}; rerun --generate after the reset",
            total_intents - done_intents
        ));
    }
    let total_enrich = enrich_jobs.len();
    let mut done_enrich = 0;
    let paused_enrich = parallel(
        enrich_jobs,
        workers,
        enrich_session,
        |(doc, hash, outcome)| {
            done_enrich += 1;
            match outcome {
                Ok(raw) => {
                    let report = raw["result"].as_str().unwrap_or_default().to_owned();
                    let parsed = parse_enrichment_report(&report).is_ok();
                    let (path, relative) = session_file(&doc, "enrichment");
                    fixtures::write_json(&path, &raw)?;
                    enrichment.docs.insert(
                        doc,
                        CachedEnrichment {
                            document_hash: hash,
                            report,
                            parsed,
                            session: relative,
                        },
                    );
                    fixtures::write_json(&fixtures::enrichment_path(), &enrichment)
                }
                Err(error) => {
                    failures.push(format!("{doc} enrichment: {error}"));
                    Ok(())
                }
            }
        },
    )?;
    fixtures::write_json(&fixtures::enrichment_path(), &enrichment)?;
    if let Some(reason) = &paused_enrich {
        record_pause(reason, total_enrich - done_enrich)?;
        return Err(format!(
            "paused before {} enrichment sessions: {reason}; rerun --generate after the reset",
            total_enrich - done_enrich
        ));
    }
    let unparsed = enrichment
        .docs
        .iter()
        .filter(|(_, c)| !c.parsed)
        .map(|(d, _)| d.as_str())
        .collect::<Vec<_>>();
    eprintln!(
        "intents cached: {}, enrichment cached: {} (unparsed: {unparsed:?})",
        intents.docs.len(),
        enrichment.docs.len()
    );
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}
