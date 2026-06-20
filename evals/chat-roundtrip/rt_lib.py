#!/usr/bin/env python3
"""Transcript assertions for the Layer B chat round-trip harness.

Kept out of run.sh because macOS bash 3.2 cannot parse here-docs nested inside
command substitution. Each subcommand reads the chat snapshot JSON on stdin
(GET /api/panes/{id}/chat) and prints a result run.sh can branch on.
"""
import json
import re
import sys
from datetime import datetime


def norm(s: str) -> str:
    """Mirror normalizeChatText: CRLF→LF, collapse whitespace, trim, lower."""
    s = (s or "").replace("\r\n", "\n")
    s = re.sub(r"\s+", " ", s)
    return s.strip().lower()


def load_stdin():
    try:
        return json.load(sys.stdin)
    except Exception:
        return None


def cmd_match_roundtrip(want_raw: str, sent_ms: int):
    """Print a capture tuple iff `want` is a user turn AND an assistant reply
    follows it; otherwise print nothing (caller keeps polling)."""
    doc = load_stdin()
    if not doc:
        return
    want = norm(want_raw)
    msgs = doc.get("messages", [])
    user_idx = -1
    user_ts = user_content = None
    for i, m in enumerate(msgs):
        if m.get("type") == "user" and norm(m.get("content", "")) == want:
            user_idx, user_ts, user_content = i, m.get("timestamp"), m.get("content")
            break
    if user_idx < 0:
        return
    replied = any(
        m.get("type") == "assistant" and (m.get("content") or "").strip()
        for m in msgs[user_idx + 1:]
    )
    if not replied:
        return
    print(json.dumps({
        "matched_user_index": user_idx,
        "user_echo_timestamp": user_ts,
        "user_echo_content": user_content,
        "client_send_ms": sent_ms,
        "assistant_replied": True,
        "total_messages": len(msgs),
    }))


def cmd_rapid_check(base: str):
    """Print 1 iff every rapid message's tokens are present across user turns."""
    doc = load_stdin()
    if not doc:
        print(0)
        return
    users = " ".join(
        norm(m.get("content", "")) for m in doc.get("messages", []) if m.get("type") == "user"
    )
    need = [norm(f"rapid {w} {base}") for w in ("one", "two", "three")]
    got = all(all(tok in users for tok in n.split()) for n in need)
    print(1 if got else 0)


def _to_ms(t):
    if t is None:
        return None
    try:
        return int(t)
    except (TypeError, ValueError):
        pass
    try:
        return int(datetime.fromisoformat(str(t).replace("Z", "+00:00")).timestamp() * 1000)
    except Exception:
        return None


def cmd_binding_state(want_raw: str):
    """Classify a chat-raw/start response (the UI seam) for the session-binding gate.

    Prints exactly one of:
      pending  — pending=true: pane.agent_session_id still empty (session NOT bound).
      empty    — bound but the wanted user echo isn't in the transcript yet.
      bound    — bound AND the user echo is present (session bound + transcript delivered).
    """
    doc = load_stdin()
    if not doc:
        print("pending")
        return
    if doc.get("pending") is True:
        print("pending")
        return
    want = norm(want_raw)
    msgs = doc.get("messages") or []
    echoed = any(
        m.get("type") == "user" and want in norm(m.get("content", "")) for m in msgs
    )
    print("bound" if echoed else "empty")


def cmd_pending_prompt(pane_id: str):
    """Read GET /api/mux/state on stdin; print the pane's extracted interactive
    prompt as JSON {question, options:[label,...]} iff one is pending, else
    nothing. This is the facet9 P9.3 oracle: did the provider's prompt get
    extracted (question + ALL options) into the existing pending_prompt pipeline?"""
    doc = load_stdin()
    if not doc:
        return
    panes = doc.get("panes") or {}
    pane = panes.get(pane_id)
    if not pane:
        return
    pp = pane.get("pending_prompt")
    if not pp:
        return
    opts = [o.get("label", "") for o in (pp.get("options") or [])]
    print(json.dumps({"question": pp.get("question", ""), "options": opts}))


def cmd_skew(capture_json: str):
    c = json.loads(capture_json)
    ts = c.get("user_echo_timestamp")
    sent = c["client_send_ms"]
    echo_ms = _to_ms(ts)
    if echo_ms is None:
        print("transcript-ts=%r (unparseable)" % ts)
        return
    print("transcript_echo_ms - client_send_ms = %+d ms (echo_ts=%s)" % (echo_ms - sent, ts))


def main():
    if len(sys.argv) < 2:
        sys.exit("usage: rt_lib.py <match-roundtrip|rapid-check|skew> ...")
    cmd = sys.argv[1]
    if cmd == "match-roundtrip":
        cmd_match_roundtrip(sys.argv[2], int(sys.argv[3]))
    elif cmd == "rapid-check":
        cmd_rapid_check(sys.argv[2])
    elif cmd == "binding-state":
        cmd_binding_state(sys.argv[2])
    elif cmd == "pending-prompt":
        cmd_pending_prompt(sys.argv[2])
    elif cmd == "skew":
        cmd_skew(sys.argv[2])
    else:
        sys.exit(f"unknown subcommand: {cmd}")


if __name__ == "__main__":
    main()
