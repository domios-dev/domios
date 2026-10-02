"""Immutable evaluator inputs and Python request domains; no runtime authenticity claim."""

from dataclasses import dataclass
import hashlib
import json
import math
from pathlib import Path
import re

ENDPOINT = "https://api.typesafe.ai/v1/systemone"
MODEL_ALIAS = "jev-latest"
SOURCE_BASIS = "e3def3cabc4d70b8ea7484ae6b27ff843ce7345d"
CANONICAL_SHA256 = "783e640d7d3e9f7f58add248cc42fb66a4a928de2fd778d9706e0c4926946f8b"
MAX_REQUEST = 256 * 1024
MAX_RESPONSE = 64 * 1024
MAX_DATASET = 16 * 1024 * 1024
MAX_CRITERIA = 64 * 1024
MAX_QUESTIONS = 32
LABELS = (
    "ready_idle", "working_normally", "provider_update_prompt", "trust_or_permission_prompt",
    "login_or_auth_required", "model_unavailable_or_quota", "crashed_to_shell", "error_loop",
    "still_starting", "other",
)


class ContractError(ValueError):
    def __init__(self, code):
        self.code = code
        super().__init__(code)


def require(condition, code):
    if not condition:
        raise ContractError(code)


def sha256(raw):
    return hashlib.sha256(raw).hexdigest()


def validate_source_sha(value):
    require(type(value) is str and re.fullmatch(r"[0-9a-f]{40}", value), "source_sha")


def _object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate_json_key")
        result[key] = value
    return result


def finite_tree(value, depth=0):
    require(depth <= 16, "json_depth")
    if value is None or type(value) in (bool, str):
        return
    if type(value) in (int, float):
        try:
            require(math.isfinite(value), "nonfinite_json")
        except OverflowError as error:
            raise ContractError("nonfinite_json") from error
        return
    if type(value) is list:
        for item in value:
            finite_tree(item, depth + 1)
        return
    require(type(value) is dict and all(type(key) is str for key in value), "json_type")
    for item in value.values():
        finite_tree(item, depth + 1)


def decode(raw, limit):
    require(type(raw) is bytes and 0 < len(raw) <= limit, "json_size")
    try:
        value = json.loads(raw, object_pairs_hook=_object,
                           parse_constant=lambda _: (_ for _ in ()).throw(ContractError("nonfinite_json")))
        finite_tree(value)
        return value
    except (UnicodeError, json.JSONDecodeError, RecursionError, OverflowError) as error:
        raise ContractError("invalid_json") from error


def encode(value, limit=MAX_REQUEST):
    finite_tree(value)
    raw = json.dumps(value, ensure_ascii=False, allow_nan=False, sort_keys=True,
                     separators=(",", ":")).encode("utf-8")
    require(0 < len(raw) <= limit, "request_size")
    return raw


def bounded_file(path, limit):
    with Path(path).open("rb") as stream:
        raw = stream.read(limit + 1)
    require(len(raw) <= limit, "input_size")
    return raw


def criteria_map(value, *, canonical=True):
    require(type(value) is dict and bool(value), "criteria_schema")
    require(set(value) == set(LABELS) if canonical else set(value) <= set(LABELS), "criteria_labels")
    require(all(type(item) is str and item.strip() for item in value.values()), "criteria_text")
    encode(value, MAX_CRITERIA)
    return value


def load_criteria_bytes(raw, *, variant=None):
    value = decode(raw, MAX_CRITERIA)
    if variant is None:
        return criteria_map(value)
    variants = value.get("variants") if type(value) is dict else None
    require(type(variants) is dict and variant in variants, "historical_variant_missing")
    return criteria_map(variants[variant], canonical=False)


def probability(value, code):
    require(type(value) in (int, float) and 0 <= value <= 1 and math.isfinite(value), code)
    return float(value)


def validate_questions(questions):
    require(type(questions) is dict and 0 < len(questions) <= MAX_QUESTIONS, "questions_schema")
    for name, question in questions.items():
        require(type(name) is str and re.fullmatch(r"[A-Za-z][A-Za-z0-9_]{0,63}", name), "question_name")
        require(type(question) is dict and set(question) == {"type", "instructions", "criteria"}, "question_schema")
        require(type(question["instructions"]) is str and question["instructions"].strip(), "question_instructions")
        criteria = question["criteria"]
        require(type(criteria) is dict and bool(criteria) and all(type(key) is str and key and
                type(value) is str and value.strip() for key, value in criteria.items()), "question_criteria")
        require(question["type"] in ("choice", "noul"), "question_type")
        if question["type"] == "noul":
            require(set(criteria) == {"true", "false"}, "noul_criteria")
    encode(questions)
    return questions


def validate_wire(raw):
    value = decode(raw, MAX_REQUEST)
    require(type(value) is dict and set(value) == {"state", "model", "questions"}, "wire_schema")
    require(type(value["model"]) is str and 0 < len(value["model"]) <= 128, "model_alias")
    validate_questions(value["questions"])
    require(type(value["state"]) in (str, dict), "state_schema")
    return value


def validate_answers(payload, questions):
    validate_questions(questions)
    answers = payload.get("answers") if type(payload) is dict else None
    require(type(answers) is dict and set(answers) == set(questions), "invalid_answer_set")
    normalized = {}
    for name, question in questions.items():
        answer = answers[name]
        require(type(answer) is dict and answer.get("type") == question["type"], "invalid_answer_shape")
        if question["type"] == "noul":
            normalized[name] = {"type": "noul", "noul": probability(answer.get("noul"), "invalid_noul")}
            continue
        choice = answer.get("choice")
        require(type(choice) is str and choice in question["criteria"], "invalid_choice")
        probabilities = answer.get("probabilities")
        require(type(probabilities) is dict and bool(probabilities) and
                set(probabilities) <= set(question["criteria"]), "invalid_probabilities")
        probabilities = {key: probability(value, "invalid_probabilities") for key, value in probabilities.items()}
        reported = answer.get("confidence")
        confidence = probabilities.get(choice) if reported is None else reported
        normalized[name] = {"type": "choice", "choice": choice,
                            "probabilities": probabilities,
                            "confidence": probability(confidence, "invalid_confidence"),
                            "confidence_source": "choice_probability" if reported is None else "reported"}
    return normalized


def wire_hashes(raw):
    wire = validate_wire(raw)
    return {"python_wire_bytes_sha256": sha256(raw),
            "python_state_json_sha256": sha256(encode(wire["state"])),
            "python_question_json_sha256": sha256(encode(wire["questions"])),
            "hash_domain": "Python UTF-8 JSON; not Rust typed-request hashes"}


@dataclass(frozen=True)
class Manifest:
    source_sha: str
    criteria_bytes: bytes
    dataset_bytes: bytes
    question_bytes: bytes
    endpoint: str
    model_alias: str
    criteria_mode: str = "canonical"

    def __post_init__(self):
        validate_source_sha(self.source_sha)
        require(self.endpoint == ENDPOINT, "endpoint_policy")
        require(type(self.model_alias) is str and 0 < len(self.model_alias) <= 128, "model_alias")
        require(type(self.dataset_bytes) is bytes and 0 < len(self.dataset_bytes) <= MAX_DATASET, "dataset_size")
        require(self.criteria_mode == "canonical", "historical_is_diagnostic_only")
        load_criteria_bytes(self.criteria_bytes)
        require(sha256(self.criteria_bytes) == CANONICAL_SHA256, "canonical_pin")
        validate_questions(decode(self.question_bytes, MAX_REQUEST))

    @property
    def questions(self):
        return decode(self.question_bytes, MAX_REQUEST)

    def report(self, samples):
        return {"source_sha": self.source_sha, "source_sha_origin": "operator_declared_unverified",
                "reviewed_source_basis": SOURCE_BASIS,
                "criteria_bytes_sha256": sha256(self.criteria_bytes),
                "dataset_bytes_sha256": sha256(self.dataset_bytes),
                "question_manifest_bytes_sha256": sha256(self.question_bytes),
                "python_question_json_sha256": sha256(encode(self.questions)),
                "declared_questions": self.questions, "endpoint": self.endpoint,
                "model_alias": self.model_alias, "immutable_server_model": False,
                **qualification_status(samples)}


def qualification_status(samples):
    sources = [sample.get("source") for sample in samples]
    if any(type(source) is str and source.startswith("synthetic") for source in sources):
        provenance = "synthetic_mechanism_fixtures"
    elif samples and all(type(source) is str and source.startswith("dev-capture") for source in sources):
        provenance = "historical_capture_claims_unverified"
    else:
        provenance = "unknown_or_current_claim_unverified"
    return {"record_provenance": provenance, "runtime_parity": "unproven",
            "current_runtime_qualification": {"eligible": False,
                "reason": "separately_reviewed_parity_and_authentic_complete_event_captures_required"},
            "automatic_actions_enabled": False}
