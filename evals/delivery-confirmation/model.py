import hashlib
import json


def canonical(text):
    return text.replace("\r\n", "\n").replace("\r", "\n").strip()


def text_values(value, path=""):
    if isinstance(value, str):
        yield path, value
    elif isinstance(value, dict):
        for key, child in value.items():
            yield from text_values(child, f"{path}.{key}".lstrip("."))
    elif isinstance(value, list):
        for index, child in enumerate(value):
            yield from text_values(child, f"{path}[{index}]")


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
            for field, text in text_values(record):
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
        group["confirmed"] += state == "delivered" and bool(row["evidence"])
        group["received_unconfirmed"] += state != "delivered" and bool(row["evidence"])
        group["states"][state] = group["states"].get(state, 0) + 1
    return [{**group, "confirmation_rate": group["confirmed"] / group["total"]}
            for group in groups.values()]
