use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use codeterm_blueprint_types::sentinel_input::{
    run_dir, within_budget, DataRef, MAX_INPUT_SOURCE_BYTES,
};

use super::InputFiles;

const FETCH_DEADLINE: Duration = Duration::from_secs(30);

pub fn materialize(
    data_dir: &Path,
    blueprint_id: &str,
    run_id: &str,
    data: &DataRef,
    deadline: Instant,
) -> Result<InputFiles, String> {
    let run_dir = run_dir(data_dir, blueprint_id, run_id)?;
    fs::create_dir_all(&run_dir).map_err(|error| format!("create input run dir: {error}"))?;
    let filename = match data {
        DataRef::Text(_) => "input.txt".to_owned(),
        DataRef::File(path) => safe_filename(path),
        DataRef::Url(url) => url
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty() && !name.contains('?') && !name.contains('#'))
            .map(sanitize_filename)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "input".into()),
    };
    let source = run_dir.join(filename);
    match data {
        DataRef::Text(text) => {
            within_budget(text.len() as u64, MAX_INPUT_SOURCE_BYTES)
                .map_err(|_| "input_oversize".to_owned())?;
            fs::write(&source, text).map_err(|error| format!("write input text: {error}"))?;
        }
        DataRef::File(path) => {
            let metadata =
                fs::metadata(path).map_err(|error| format!("input_file_missing: {error}"))?;
            if !metadata.is_file() {
                return Err("input_file_missing: source is not a regular file".into());
            }
            within_budget(metadata.len(), MAX_INPUT_SOURCE_BYTES)
                .map_err(|_| "input_oversize".to_owned())?;
            fs::copy(path, &source).map_err(|error| format!("materialize input file: {error}"))?;
            within_budget(
                fs::metadata(&source)
                    .map_err(|error| format!("inspect materialized input: {error}"))?
                    .len(),
                MAX_INPUT_SOURCE_BYTES,
            )
            .map_err(|_| "input_oversize".to_owned())?;
        }
        DataRef::Url(url) => fetch_url(url, &source, deadline)?,
    }
    Ok(InputFiles { source })
}

fn fetch_url(url: &str, destination: &Path, deadline: Instant) -> Result<(), String> {
    fetch_url_with_budget(url, destination, deadline, MAX_INPUT_SOURCE_BYTES)
}

fn fetch_url_with_budget(
    url: &str,
    destination: &Path,
    deadline: Instant,
    budget: u64,
) -> Result<(), String> {
    let request_deadline = deadline.min(Instant::now() + FETCH_DEADLINE);
    let remaining = request_deadline.saturating_duration_since(Instant::now());
    if remaining.is_zero() {
        return Err("input_deadline_exceeded".into());
    }
    let client = BlueprintInputHttpClient::new(remaining)?;
    let mut response = client
        .0
        .get(url)
        .send()
        .map_err(|_| "input_url_fetch_failed".to_owned())?
        .error_for_status()
        .map_err(|_| "input_url_fetch_failed".to_owned())?;
    if let Some(length) = response.content_length() {
        within_budget(length, budget).map_err(|_| "input_oversize".to_owned())?;
    }
    let mut output =
        File::create(destination).map_err(|error| format!("create URL input: {error}"))?;
    let mut buffer = [0u8; 16 * 1024];
    let mut size = 0u64;
    loop {
        if Instant::now() >= request_deadline {
            return Err("input_deadline_exceeded".into());
        }
        let read = response
            .read(&mut buffer)
            .map_err(|_| "input_url_read_failed".to_owned())?;
        if read == 0 {
            return Ok(());
        }
        size = size.saturating_add(read as u64);
        within_budget(size, budget).map_err(|_| "input_oversize".to_owned())?;
        output
            .write_all(&buffer[..read])
            .map_err(|error| format!("write URL input: {error}"))?;
    }
}

struct BlueprintInputHttpClient(reqwest::blocking::Client);

impl BlueprintInputHttpClient {
    fn new(timeout: Duration) -> Result<Self, String> {
        reqwest::blocking::Client::builder()
            .timeout(timeout)
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map(Self)
            .map_err(|error| format!("create blueprint input HTTP client: {error}"))
    }
}

fn safe_filename(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(sanitize_filename)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "input".into())
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .filter(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')
        })
        .take(120)
        .collect()
}

pub fn sweep_orphans(data_dir: &Path) -> Result<usize, String> {
    let root = data_dir.join("assets").join("blueprint-runs");
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            log::info!("[blueprint-input] startup orphan sweep removed 0 run directories");
            return Ok(0);
        }
        Err(error) => return Err(format!("read blueprint input run root: {error}")),
    };
    let mut removed = 0;
    for blueprint in entries {
        let blueprint =
            blueprint.map_err(|error| format!("read blueprint input entry: {error}"))?;
        let path: PathBuf = blueprint.path();
        if !path.is_dir() {
            continue;
        }
        for run in fs::read_dir(&path).map_err(|error| format!("read blueprint runs: {error}"))? {
            let run = run.map_err(|error| format!("read blueprint run: {error}"))?;
            if run.path().is_dir() {
                fs::remove_dir_all(run.path())
                    .map_err(|error| format!("remove orphan blueprint run: {error}"))?;
                removed += 1;
            }
        }
    }
    log::info!("[blueprint-input] startup orphan sweep removed {removed} run directories");
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blueprint_input_materialize_file_copies_into_run_dir() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("voice.wav");
        fs::write(&source, b"audio").unwrap();
        let files = materialize(
            temp.path(),
            "blueprint_1",
            "run_1",
            &DataRef::File(source),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap();
        assert_eq!(fs::read(files.source).unwrap(), b"audio");
    }

    #[test]
    fn blueprint_input_materialize_missing_file_refuses() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing.wav");
        let error = materialize(
            temp.path(),
            "blueprint_1",
            "run_1",
            &DataRef::File(missing),
            Instant::now() + Duration::from_secs(5),
        )
        .unwrap_err();
        assert!(error.starts_with("input_file_missing:"));
    }

    #[test]
    fn blueprint_input_materialize_url_size_cap_refuses_without_writing_body() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 1024];
            let _ = stream.read(&mut request).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\nConnection: close\r\n\r\nabcdef",
                )
                .unwrap();
        });
        let temp = tempfile::tempdir().unwrap();
        let destination = temp.path().join("input");
        assert_eq!(
            fetch_url_with_budget(
                &format!("http://{address}/voice.wav"),
                &destination,
                Instant::now() + Duration::from_secs(5),
                5,
            )
            .unwrap_err(),
            "input_oversize"
        );
        server.join().unwrap();
        assert!(!destination.exists());
    }

    #[test]
    fn blueprint_input_sweep_removes_orphan_runs_and_reports_count() {
        let temp = tempfile::tempdir().unwrap();
        let run = run_dir(temp.path(), "blueprint_1", "run_1").unwrap();
        fs::create_dir_all(&run).unwrap();
        assert_eq!(sweep_orphans(temp.path()).unwrap(), 1);
        assert!(!run.exists());
    }
}
