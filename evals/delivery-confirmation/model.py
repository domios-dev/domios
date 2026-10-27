import hashlib
import json


def canonical(text):
    return text.replace("\r\n", "\n").replace("\r", "\n").strip()


def text_segments(content, path):
    if isinstance(content, str):
        yield path, content
    elif isinstance(content, list):
        for index, part in enumerate(content):
            if isinstance(part, dict) and part.get("type") in ("text", "input_text") and isinstance(part.get("text"), str):
                yield f"{path}[{index}].text", part["text"]


def consumed_text_values(record):
    attachment = record.get("attachment") or {}
    if record.get("type") == "attachment" and attachment.get("type") == "queued_command":
        prompt = attachment.get("prompt")
        if isinstance(prompt, str) and attachment.get("commandMode") != "task-notification":
            yield "attachment.prompt", prompt
        return
    message = record.get("message") if record.get("type") in ("user", "assistant") else None
    path = "message.content"
    if record.get("type") == "response_item":
        message = record.get("payload")
        path = "payload.content"
    if not isinstance(message, dict) or record.get("isMeta"):
        return
    content = message.get("content")
    if message.get("role") == "user":
        yield from text_segments(content, path)
    if isinstance(content, list):
        for index, part in enumerate(content):
            if isinstance(part, dict) and part.get("type") == "tool_result":
                yield from text_segments(part.get("content"), f"{path}[{index}].content")


def matching_records(transcript, anchor, session_id, payload):
    expected = canonical(payload)
    if not expected:
        return []
    matches = []
    with open(transcript, "rb") as stream:
        stream.seek(anchor)
        while True:
            offset = stream.tell()
            line = stream.readline()
            if not line or not line.endswith(b"\n"):
                break
            record = json.loads(line)
            declared = record.get("sessionId", record.get("session_id"))
            if declared is not None and declared != session_id:
                continue
            for field, text in consumed_text_values(record):
                if expected in canonical(text):
                    matches.append({"offset": offset, "field": field,
                                    "record_id": record.get("uuid", record.get("id")),
                                    "payload_sha256": hashlib.sha256(payload.encode()).hexdigest(),
                                    "record": record})
                    break
    return matches


def summarize(rows):
    groups = {}
    for row in rows:
        key = (row["provider"], row["path"], row["mode"])
        group = groups.setdefault(key, {"provider": key[0], "path": key[1],
                                      "mode": key[2], "total": 0, "confirmed": 0,
                                      "received_unconfirmed": 0, "states": {}})
        state = row["state"]
        group["total"] += 1
        group["confirmed"] += state == "delivered" and len(row["evidence"]) == 1
        group["received_unconfirmed"] += state != "delivered" and len(row["evidence"]) == 1
        group["states"][state] = group["states"].get(state, 0) + 1
    return [{**group, "confirmation_rate": group["confirmed"] / group["total"]}
            for group in groups.values()]
