use serde_json::{json, Value};

use crate::{
    args::live_transcript::{LiveTranscriptAction as Action, TranscriptAdvance, TranscriptOwner},
    client::SocketClient,
    cmd::print::{fail, fail_http, print_json, print_object},
    local_api::LocalApiClient,
};

pub fn handle(client: SocketClient, json_output: bool, action: Action) {
    let http = LocalApiClient::from_rpc(client);
    let (owner, verb, query, body) = request(action);
    let mut path = format!(
        "/api/live-transcript/{}/{}",
        urlencoding::encode(&owner.tab),
        verb
    );
    if !query.is_empty() {
        path.push('?');
        path.push_str(&query);
    }
    let response = match body {
        Some(body) => http.post(path).json(&body).send(),
        None => http.get(path).send(),
    }
    .unwrap_or_else(|error| fail(1, error.to_string()));
    if !response.status().is_success() {
        let status = response.status().as_u16();
        let body = response.text().unwrap_or_default();
        fail_http(1, status, &body);
    }
    let response: Value = response
        .json()
        .unwrap_or_else(|error| fail(1, error.to_string()));
    if json_output {
        print_json(&response);
    } else if let Some(text) = response.get("text").and_then(Value::as_str) {
        println!("{text}");
        if let Some(next) = response.get("next").filter(|next| next.is_object()) {
            print_object(next);
        }
    } else {
        print_object(&response);
    }
}

fn request(action: Action) -> (TranscriptOwner, &'static str, String, Option<Value>) {
    use Action::*;
    match action {
        Status(owner) => get(owner, "status", String::new()),
        Overview(owner) => get(owner, "overview", String::new()),
        Read {
            owner,
            revision,
            from_seq,
            through_seq,
            char_offset,
            max_chars,
        } => get(
            owner,
            "read",
            query([
                ("revision", revision),
                ("fromSeq", from_seq),
                ("throughSeq", through_seq),
                ("charOffset", char_offset.map(|offset| offset as u64)),
                ("maxChars", max_chars.map(|chars| chars as u64)),
            ]),
        ),
        Search {
            owner,
            query: search,
            from_seq,
            limit,
        } => {
            let page = query([
                ("fromSeq", from_seq),
                ("limit", limit.map(|limit| limit as u64)),
            ]);
            let mut search = format!("query={}", urlencoding::encode(&search));
            if !page.is_empty() {
                search.push('&');
                search.push_str(&page);
            }
            get(owner, "search", search)
        }
        Ack { owner, advance } => advance_cursor(owner, "ack", advance),
        MarkSeen { owner, advance } => advance_cursor(owner, "mark-seen", advance),
        Attach {
            owner,
            external_tab,
            trigger,
            mode_only,
            from_beginning,
        } => post(
            owner,
            "attach",
            json!({
                "externalTab": external_tab, "trigger": trigger, "modeOnly": mode_only,
                "start": from_beginning.then_some("beginning"),
            }),
        ),
        Pause {
            owner,
            client,
            lease,
            sequence,
        } => post(
            owner,
            "pause",
            json!({ "client": client, "lease": lease, "sequence": sequence }),
        ),
        Resume { owner, client } => post(owner, "resume", json!({ "client": client })),
    }
}

fn get(
    owner: TranscriptOwner,
    verb: &'static str,
    query: String,
) -> (TranscriptOwner, &'static str, String, Option<Value>) {
    (owner, verb, query, None)
}

fn post(
    owner: TranscriptOwner,
    verb: &'static str,
    body: Value,
) -> (TranscriptOwner, &'static str, String, Option<Value>) {
    (owner, verb, String::new(), Some(body))
}

fn advance_cursor(
    owner: TranscriptOwner,
    verb: &'static str,
    advance: TranscriptAdvance,
) -> (TranscriptOwner, &'static str, String, Option<Value>) {
    post(
        owner,
        verb,
        json!({ "revision": advance.revision, "throughSeq": advance.through_seq }),
    )
}

fn query<const N: usize>(fields: [(&str, Option<u64>); N]) -> String {
    fields
        .into_iter()
        .filter_map(|(key, value)| value.map(|value| format!("{key}={value}")))
        .collect::<Vec<_>>()
        .join("&")
}
