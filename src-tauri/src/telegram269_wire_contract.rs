use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use ed25519_dalek::{Signer, SigningKey, Verifier};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

const VECTORS: &str = include_str!("../tests/fixtures/telegram269-mobile-core-wire-v1.json");
const DAY_MS: u64 = 86_400_000;

fn u32be(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}
fn u64be(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}
fn b(out: &mut Vec<u8>, value: &[u8]) {
    u32be(out, value.len() as u32);
    out.extend_from_slice(value);
}
fn field(value: &Value, key: &str) -> Vec<u8> {
    value[key].as_str().unwrap().as_bytes().to_vec()
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn sha256(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
fn unhex(value: &str) -> Vec<u8> {
    assert_eq!(value.len() % 2, 0);
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn alphabet_value(byte: u8) -> Option<u8> {
    match byte {
        b'A'..=b'Z' => Some(byte - b'A'),
        b'a'..=b'z' => Some(byte - b'a' + 26),
        b'0'..=b'9' => Some(byte - b'0' + 52),
        b'-' => Some(62),
        b'_' => Some(63),
        _ => None,
    }
}

fn decode_base64url(value: &str) -> Result<Vec<u8>, &'static str> {
    if value.is_empty() {
        return Err("empty");
    }
    if !value.is_ascii() || value.bytes().any(|byte| alphabet_value(byte).is_none()) {
        return Err("nonalphabet");
    }
    match value.len() % 4 {
        1 => return Err("remainder_one"),
        2 if alphabet_value(*value.as_bytes().last().unwrap()).unwrap() & 0x0f != 0 => {
            return Err("nonzero_unused_bits")
        }
        3 if alphabet_value(*value.as_bytes().last().unwrap()).unwrap() & 0x03 != 0 => {
            return Err("nonzero_unused_bits")
        }
        _ => {}
    }
    URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| "invalid_encoding")
}

fn digest_text(value: &Value, key: &str) -> [u8; 32] {
    unhex(
        value[key]
            .as_str()
            .unwrap()
            .strip_prefix("sha256:")
            .unwrap(),
    )
    .try_into()
    .unwrap()
}

fn target_bytes(target: &Value) -> Vec<u8> {
    let mut out = b"CTMC1-TARGET\0".to_vec();
    match target["kind"].as_str().unwrap() {
        "none" => out.push(0),
        "tab" => {
            out.push(1);
            b(&mut out, &field(target, "gateway_instance_id"));
            if target["peer_id"].is_null() {
                out.push(0);
            } else {
                out.push(1);
                b(&mut out, &field(target, "peer_id"));
            }
            for key in ["target_instance_id", "tab_id", "session_id"] {
                b(&mut out, &field(target, key));
            }
            u64be(&mut out, target["lifecycle_epoch"].as_u64().unwrap());
            out.extend_from_slice(&unhex(target["binding_nonce_digest_hex"].as_str().unwrap()));
        }
        other => panic!("unknown target kind {other}"),
    }
    out
}

fn request_signature_bytes(vector: &Value) -> Vec<u8> {
    let mut out = b"CTMC1-REQUEST\0".to_vec();
    for key in [
        "proof_type",
        "client_id",
        "key_fingerprint",
        "server_instance_id",
        "endpoint_id",
        "method",
        "canonical_target",
    ] {
        b(&mut out, &field(vector, key));
    }
    out.extend_from_slice(&digest_text(vector, "body_sha256"));
    u64be(&mut out, vector["issued_at_ms"].as_u64().unwrap());
    u64be(&mut out, vector["expires_at_ms"].as_u64().unwrap());
    out.extend_from_slice(&decode_base64url(vector["nonce_128"].as_str().unwrap()).unwrap());
    out
}

fn discovery_bytes(vector: &Value) -> Vec<u8> {
    let mut out = b"CTMC1-DISCOVERY\0".to_vec();
    for key in [
        "endpoint_id",
        "origin",
        "transport",
        "server_instance_id",
        "server_key_id",
        "server_key_fingerprint",
        "tls_spki_sha256",
        "protocol_version",
    ] {
        b(&mut out, &field(vector, key));
    }
    u64be(&mut out, vector["directory_revision"].as_u64().unwrap());
    b(&mut out, &field(vector, "build_sha"));
    out.extend_from_slice(&unhex(vector["server_public_key_hex"].as_str().unwrap()));
    out
}

fn response_bytes(vector: &Value) -> Vec<u8> {
    let mut out = b"CTMC1-RESPONSE\0".to_vec();
    for key in ["status", "request_method", "canonical_target"] {
        b(&mut out, &field(vector, key));
    }
    out.extend_from_slice(&decode_base64url(vector["request_nonce"].as_str().unwrap()).unwrap());
    for key in [
        "request_id",
        "client_id",
        "server_instance_id",
        "server_key_id",
    ] {
        b(&mut out, &field(vector, key));
    }
    out.extend_from_slice(&digest_text(vector, "request_body_sha256"));
    out.extend_from_slice(&digest_text(vector, "response_body_sha256"));
    out.extend_from_slice(&decode_base64url(vector["response_nonce"].as_str().unwrap()).unwrap());
    u64be(&mut out, vector["server_time_ms"].as_u64().unwrap());
    out
}

fn sse_seed_input(vector: &Value) -> Vec<u8> {
    let mut out = b"CTMC1-SSE-SEED\0".to_vec();
    for key in [
        "subscription_id",
        "boot_id",
        "client_id",
        "server_instance_id",
        "endpoint_id",
    ] {
        b(&mut out, &field(vector, key));
    }
    out.extend_from_slice(&digest_text(vector, "target_set_sha256"));
    u64be(&mut out, vector["grant_revision"].as_u64().unwrap());
    out
}

fn sse_frame_bytes(vector: &Value, previous_hash: &[u8; 32]) -> Vec<u8> {
    let mut out = b"CTMC1-SSE-FRAME\0".to_vec();
    u64be(&mut out, vector["sequence"].as_u64().unwrap());
    out.extend_from_slice(previous_hash);
    for key in [
        "subscription_id",
        "client_id",
        "server_instance_id",
        "endpoint_id",
    ] {
        b(&mut out, &field(vector, key));
    }
    b(&mut out, vector["data_jcs"].as_str().unwrap().as_bytes());
    out
}

fn sse_signature_input(frame: &[u8], nonce: &[u8]) -> Vec<u8> {
    [b"CTMC1-SSE-SIGNATURE\0".as_slice(), frame, nonce].concat()
}

#[derive(Clone, Debug, PartialEq)]
enum RequestState {
    Reserved,
    Submitted,
    Unknown,
}

#[derive(Clone)]
struct Reservation {
    digest: [u8; 32],
    target_digest: [u8; 32],
    expires_at: u64,
    state: RequestState,
}

#[derive(Default, Clone)]
struct ReservationStore {
    active: HashMap<(String, String), Reservation>,
    tombstones: HashMap<(String, String), ([u8; 32], [u8; 32])>,
    submissions: usize,
}

impl ReservationStore {
    fn reserve(
        &mut self,
        client: &str,
        id: &str,
        digest: [u8; 32],
        target_digest: [u8; 32],
        now: u64,
    ) -> Result<RequestState, &'static str> {
        let key = (client.to_owned(), id.to_owned());
        if let Some((old_digest, _)) = self.tombstones.get(&key) {
            return if old_digest == &digest {
                Err("reservation_expired")
            } else {
                Err("client_request_reuse")
            };
        }
        if let Some(record) = self.active.get(&key) {
            if now < record.expires_at {
                return if record.digest == digest && record.target_digest == target_digest {
                    Ok(record.state.clone())
                } else {
                    Err("client_request_reuse")
                };
            }
            let old = self.active.remove(&key).unwrap();
            self.tombstones.insert(key, (old.digest, old.target_digest));
            return if old.digest == digest {
                Err("reservation_expired")
            } else {
                Err("client_request_reuse")
            };
        }
        self.active.insert(
            key,
            Reservation {
                digest,
                target_digest,
                expires_at: now + DAY_MS,
                state: RequestState::Reserved,
            },
        );
        self.submissions += 1;
        Ok(RequestState::Reserved)
    }
    fn recover(&mut self) {
        for record in self.active.values_mut() {
            if matches!(
                record.state,
                RequestState::Reserved | RequestState::Submitted
            ) {
                record.state = RequestState::Unknown;
            }
        }
    }
    fn status(
        &self,
        caller: &str,
        owner: &str,
        id: &str,
        grant: bool,
        active: bool,
    ) -> Result<RequestState, &'static str> {
        if !active {
            return Err("client_revoked");
        }
        if caller != owner {
            return Err("request_not_found");
        }
        if !grant {
            return Err("grant_denied");
        }
        self.active
            .get(&(owner.into(), id.into()))
            .map(|record| record.state.clone())
            .ok_or("request_not_found")
    }
}

fn receiver_authorizes(
    target: &Value,
    instance: &str,
    epoch: u64,
    configured: &str,
    reported: &str,
) -> bool {
    target["kind"] == "tab"
        && target["target_instance_id"] == instance
        && target["lifecycle_epoch"] == epoch
        && configured == reported
        && unhex(target["binding_nonce_digest_hex"].as_str().unwrap())
            == sha256(configured.as_bytes())
}

fn canonical_jcs(text: &str) -> bool {
    serde_json::to_string(&serde_json::from_str::<Value>(text).unwrap()).unwrap() == text
}

fn percent_encode(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (*byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn canonical_target(path: &str, pairs: &[(String, String)]) -> String {
    let mut encoded: Vec<_> = pairs
        .iter()
        .map(|(key, value)| (percent_encode(key), percent_encode(value)))
        .collect();
    encoded.sort();
    let query = encoded
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&");
    if query.is_empty() {
        path.to_owned()
    } else {
        format!("{path}?{query}")
    }
}

fn authorize_proof_type(proof_type: &str, normal_route: bool) -> Result<(), &'static str> {
    if normal_route && proof_type != "active" {
        Err("prospective_key_not_allowed")
    } else {
        Ok(())
    }
}

fn resume_cursor(current_boot: &str, cursor: &str) -> Result<u64, &'static str> {
    let (boot, sequence) = cursor.rsplit_once('.').ok_or("cursor_reset")?;
    if boot != current_boot {
        return Err("cursor_reset");
    }
    sequence.parse().map_err(|_| "cursor_reset")
}

fn directory_limit(limit: u64) -> Result<u64, &'static str> {
    if (1..=64).contains(&limit) {
        Ok(limit)
    } else {
        Err("body_too_large")
    }
}

fn produce_watch(authenticated_client: &str, body: &Value) -> Result<Value, &'static str> {
    if body["parent_client_id"] != authenticated_client
        || body.as_object().unwrap().keys().any(|key| {
            matches!(
                key.as_str(),
                "client_public_key" | "grants" | "machine_capabilities"
            )
        })
    {
        return Err("grant_denied");
    }
    Ok(body.clone())
}

fn validate_message(message: &Value) -> Result<(), &'static str> {
    const TYPES: [&str; 5] = ["text", "thinking", "tool_call", "tool_result", "system"];
    const UNSUPPORTED: [&str; 6] = [
        "attachments",
        "tool_input",
        "tool_rich_result",
        "file_diffs",
        "interactive_elements",
        "provider_specific_block",
    ];
    if !TYPES.contains(&message["type"].as_str().unwrap_or(""))
        || message["content"]
            .as_str()
            .map_or(true, |v| v.len() > 65_536)
    {
        return Err("unsupported_message_family");
    }
    if message["unsupported_families"]
        .as_array()
        .ok_or("unsupported_message_family")?
        .iter()
        .any(|v| !UNSUPPORTED.contains(&v.as_str().unwrap_or("")))
    {
        return Err("unsupported_message_family");
    }
    if let Some(tool) = message["tool_call"].as_object() {
        let mut keys: Vec<_> = tool.keys().map(String::as_str).collect();
        keys.sort_unstable();
        if keys != ["detail", "error", "id", "name", "summary"] {
            return Err("unsupported_message_family");
        }
    }
    Ok(())
}

fn validate_theme(theme: &Value) -> Result<(), &'static str> {
    const COLORS: [&str; 20] = [
        "background",
        "foreground",
        "cursor",
        "selection",
        "black",
        "red",
        "green",
        "yellow",
        "blue",
        "magenta",
        "cyan",
        "white",
        "bright_black",
        "bright_red",
        "bright_green",
        "bright_yellow",
        "bright_blue",
        "bright_magenta",
        "bright_cyan",
        "bright_white",
    ];
    let object = theme.as_object().ok_or("unsupported_theme")?;
    if object.len() != 21 || COLORS.iter().any(|key| !object.contains_key(*key)) {
        return Err("unsupported_theme");
    }
    for (key, value) in object {
        let text = value.as_str().ok_or("unsupported_theme")?;
        if key == "name" {
            if text.chars().count() > 128 {
                return Err("unsupported_theme");
            }
        } else if text.len() != 7
            || !text.starts_with('#')
            || !text[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("unsupported_theme");
        }
    }
    Ok(())
}

#[test]
fn telegram269_wire_contract() {
    let vectors: Value = serde_json::from_str(VECTORS).unwrap();
    let key = SigningKey::from_bytes(
        &unhex(vectors["keys"]["private_seed_hex"].as_str().unwrap())
            .try_into()
            .unwrap(),
    );
    assert_eq!(
        hex(key.verifying_key().as_bytes()),
        vectors["keys"]["public_key_hex"]
    );
    for vector in vectors["base64url"]["valid"].as_array().unwrap() {
        assert_eq!(
            decode_base64url(vector["encoded"].as_str().unwrap()).unwrap(),
            unhex(vector["hex"].as_str().unwrap())
        );
    }
    for vector in vectors["base64url"]["invalid"].as_array().unwrap() {
        assert_eq!(
            decode_base64url(vector["encoded"].as_str().unwrap()),
            Err(vector["error"].as_str().unwrap())
        );
    }

    let target = &vectors["target"];
    assert_eq!(hex(&target_bytes(target)), target["expected_hex"]);
    assert_eq!(
        target["binding_nonce_digest_hex"],
        hex(&sha256(
            vectors["authority"]["session_binding_nonce"]
                .as_str()
                .unwrap()
                .as_bytes()
        ))
    );
    assert!(receiver_authorizes(
        target,
        "remote-b",
        42,
        "binding-live-29",
        "binding-live-29"
    ));
    assert!(!receiver_authorizes(
        target,
        "gateway-a",
        42,
        "binding-live-29",
        "binding-live-29"
    ));
    assert!(!receiver_authorizes(
        target,
        "remote-b",
        43,
        "binding-live-29",
        "binding-live-29"
    ));
    assert!(!receiver_authorizes(
        target,
        "remote-b",
        42,
        "binding-live-29",
        "binding-old"
    ));
    assert_eq!(target_bytes(&vectors["none_target"]), b"CTMC1-TARGET\0\0");

    let request = &vectors["request"];
    assert!(canonical_jcs(
        request["unsigned_proof_jcs"].as_str().unwrap()
    ));
    assert!(!canonical_jcs(
        request["noncanonical_proof_jcs"].as_str().unwrap()
    ));
    let request_wire = request_signature_bytes(request);
    assert_eq!(hex(&request_wire), request["expected_hex"]);
    let request_signature = key.sign(&request_wire);
    assert_eq!(
        hex(&request_signature.to_bytes()),
        request["expected_signature_hex"]
    );
    key.verifying_key()
        .verify(&request_wire, &request_signature)
        .unwrap();
    let mut changed = request_wire.clone();
    *changed.last_mut().unwrap() ^= 1;
    assert!(key
        .verifying_key()
        .verify(&changed, &request_signature)
        .is_err());
    assert_eq!(
        authorize_proof_type("pairing", true),
        Err("prospective_key_not_allowed")
    );
    let query = &vectors["canonical_target"];
    let pairs = query["pairs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pair| {
            (
                pair[0].as_str().unwrap().to_owned(),
                pair[1].as_str().unwrap().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        canonical_target(query["path"].as_str().unwrap(), &pairs),
        query["expected"]
    );
    assert_eq!(
        serde_json::to_string(&vectors["jcs"]["value"]).unwrap(),
        vectors["jcs"]["expected"]
    );

    let discovery = &vectors["discovery"];
    let discovery_wire = discovery_bytes(discovery);
    assert_eq!(hex(&discovery_wire), discovery["expected_hex"]);
    let discovery_signature = key.sign(&discovery_wire);
    assert_eq!(
        hex(&discovery_signature.to_bytes()),
        discovery["expected_signature_hex"]
    );
    key.verifying_key()
        .verify(&discovery_wire, &discovery_signature)
        .unwrap();

    let response = &vectors["response"];
    assert!(canonical_jcs(response["body_jcs"].as_str().unwrap()));
    assert!(!response["body_jcs"].as_str().unwrap().contains("signature"));
    assert_eq!(
        hex(&sha256(response["body_jcs"].as_str().unwrap().as_bytes())),
        response["response_body_sha256"]
            .as_str()
            .unwrap()
            .strip_prefix("sha256:")
            .unwrap()
    );
    let response_wire = response_bytes(response);
    assert_eq!(hex(&response_wire), response["expected_hex"]);
    let response_signature = key.sign(&response_wire);
    assert_eq!(
        hex(&response_signature.to_bytes()),
        response["expected_signature_hex"]
    );
    key.verifying_key()
        .verify(&response_wire, &response_signature)
        .unwrap();
    let mut changed_body = response.clone();
    changed_body["response_body_sha256"] = json!(format!("sha256:{}", hex(&sha256(b"{}"))));
    let changed_response = response_bytes(&changed_body);
    assert_ne!(changed_response, response_wire);
    assert!(key
        .verifying_key()
        .verify(&changed_response, &response_signature)
        .is_err());

    let sse = &vectors["sse"];
    assert_eq!(
        sse["subscription_route"],
        "/api/mobile/v1/events/subscriptions"
    );
    assert_eq!(
        sse["stream_route"],
        "/api/mobile/v1/events/stream/sub-01?cursor=boot-01.7"
    );
    assert!(sse["stream_route"].as_str().unwrap().len() <= 2048);
    assert_eq!(
        target_bytes(&sse["subscription_body"]["targets"][0]),
        target_bytes(target)
    );
    assert_eq!(
        hex(&sha256(
            sse["target_array_jcs"].as_str().unwrap().as_bytes()
        )),
        sse["target_set_sha256"]
            .as_str()
            .unwrap()
            .strip_prefix("sha256:")
            .unwrap()
    );
    let seed_input = sse_seed_input(sse);
    assert_eq!(hex(&seed_input), sse["expected_seed_input_hex"]);
    let seed = sha256(&seed_input);
    assert_eq!(hex(&seed), sse["expected_seed_hex"]);
    let first = sse_frame_bytes(&sse["frames"][0], &seed);
    assert_eq!(hex(&first), sse["frames"][0]["expected_frame_hex"]);
    let nonce = decode_base64url(sse["frames"][0]["response_nonce"].as_str().unwrap()).unwrap();
    let first_signature_input = sse_signature_input(&first, &nonce);
    let first_signature = key.sign(&first_signature_input);
    assert_eq!(
        hex(&first_signature.to_bytes()),
        sse["frames"][0]["expected_signature_hex"]
    );
    key.verifying_key()
        .verify(&first_signature_input, &first_signature)
        .unwrap();
    assert!(!sse["frames"][0]["data_jcs"]
        .as_str()
        .unwrap()
        .contains("signature"));
    assert!(!sse["frames"][0]["data_jcs"]
        .as_str()
        .unwrap()
        .contains("previous_frame_hash"));
    let previous_hash = sha256(&first);
    let second = sse_frame_bytes(&sse["frames"][1], &previous_hash);
    assert_eq!(hex(&second), sse["frames"][1]["expected_frame_hex"]);
    let nonce = decode_base64url(sse["frames"][1]["response_nonce"].as_str().unwrap()).unwrap();
    assert_eq!(
        hex(&key.sign(&sse_signature_input(&second, &nonce)).to_bytes()),
        sse["frames"][1]["expected_signature_hex"]
    );
    let mut broken = previous_hash;
    broken[0] ^= 1;
    assert_ne!(sse_frame_bytes(&sse["frames"][1], &broken), second);
    let mut narrowed = sse.clone();
    narrowed["grant_revision"] = json!(8);
    assert_ne!(sha256(&sse_seed_input(&narrowed)), seed);
    assert_eq!(
        resume_cursor("boot-01", "boot-other.7"),
        Err("cursor_reset")
    );
    assert_eq!(resume_cursor("boot-01", "boot-01.7"), Ok(7));

    let send = &vectors["send"];
    assert_eq!(
        send["route"],
        "/api/mobile/v1/gateways/gateway-a/targets/remote-b/tabs/tab-17/sends"
    );
    assert!(canonical_jcs(send["body_jcs"].as_str().unwrap()));
    let digest = sha256(send["body_jcs"].as_str().unwrap().as_bytes());
    let target_digest = sha256(&target_bytes(target));
    let mut store = ReservationStore::default();
    assert_eq!(
        store.reserve("mc_alpha", "send-01", digest, target_digest, 1_000),
        Ok(RequestState::Reserved)
    );
    assert_eq!(
        store.reserve("mc_alpha", "send-01", digest, target_digest, 1_001),
        Ok(RequestState::Reserved)
    );
    assert_eq!(store.submissions, 1);
    assert_eq!(
        store.reserve(
            "mc_alpha",
            "send-01",
            sha256(b"different"),
            target_digest,
            1_002
        ),
        Err("client_request_reuse")
    );
    store
        .active
        .get_mut(&("mc_alpha".into(), "send-01".into()))
        .unwrap()
        .state = RequestState::Submitted;
    store.recover();
    assert_eq!(
        store.status("mc_alpha", "mc_alpha", "send-01", true, true),
        Ok(RequestState::Unknown)
    );
    assert_eq!(
        store.reserve("mc_alpha", "send-01", digest, target_digest, 2_000),
        Ok(RequestState::Unknown)
    );
    assert_eq!(store.submissions, 1);
    assert_eq!(
        store.status("other", "mc_alpha", "send-01", true, true),
        Err("request_not_found")
    );
    assert_eq!(
        store.status("mc_alpha", "mc_alpha", "send-01", false, true),
        Err("grant_denied")
    );
    assert_eq!(
        store.status("mc_alpha", "mc_alpha", "send-01", true, false),
        Err("client_revoked")
    );
    assert_eq!(
        store.reserve("mc_alpha", "send-01", digest, target_digest, 1_000 + DAY_MS),
        Err("reservation_expired")
    );
    let mut restored = store.clone();
    assert_eq!(
        restored.reserve(
            "mc_alpha",
            "send-01",
            digest,
            target_digest,
            1_000 + DAY_MS + 1
        ),
        Err("reservation_expired")
    );
    assert_eq!(
        store.reserve(
            "mc_alpha",
            "send-01",
            sha256(b"different"),
            target_digest,
            1_000 + DAY_MS + 1
        ),
        Err("client_request_reuse")
    );
    assert_eq!(store.submissions, 1);

    assert!(validate_message(&vectors["schemas"]["message"]).is_ok());
    assert_eq!(
        validate_message(&vectors["schemas"]["unsupported_message"]),
        Err("unsupported_message_family")
    );
    assert!(validate_theme(&vectors["schemas"]["theme"]).is_ok());
    assert_eq!(
        validate_theme(&vectors["schemas"]["unsupported_theme"]),
        Err("unsupported_theme")
    );
    assert_eq!(directory_limit(64), Ok(64));
    assert_eq!(directory_limit(65), Err("body_too_large"));
    assert_eq!(vectors["limits"]["subscription_targets"], 64);
    let watch = &vectors["watch"];
    assert!(produce_watch(
        watch["authenticated_client_id"].as_str().unwrap(),
        &watch["body"]
    )
    .is_ok());
    let mut foreign_watch = watch["body"].clone();
    foreign_watch["parent_client_id"] = json!("other");
    assert_eq!(
        produce_watch("mc_alpha", &foreign_watch),
        Err("grant_denied")
    );
    let mut privileged_watch = watch["body"].clone();
    privileged_watch["grants"] = json!(["chat.send"]);
    assert_eq!(
        produce_watch("mc_alpha", &privileged_watch),
        Err("grant_denied")
    );
    assert!(watch["rejected_claims"]
        .as_array()
        .unwrap()
        .contains(&json!("client_public_key")));
    assert!(watch["rejected_claims"]
        .as_array()
        .unwrap()
        .contains(&json!("grants")));
}
