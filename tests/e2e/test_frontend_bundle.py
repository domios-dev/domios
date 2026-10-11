"""Focused checkpoint API proof; requires canonical DEV with lane 5 wired."""

import uuid


def test_frontend_checkpoint_roundtrip(api):
    client = "frontend-fixture-" + uuid.uuid4().hex
    path = f"/api/frontend/clients/{client}/checkpoint"
    value = {
        "revision": 7,
        "payload": {
            "activeWorkspaceId": "workspace-a",
            "activeTabId": "tab-a",
            "drafts": {
                "tab-a": {
                    "text": "exact unsent draft",
                    "attachmentIds": ["attachment-a", "attachment-b"],
                    "replyTo": None,
                }
            },
            "scroll": {"tab-a": {"terminal": "line-42", "chat": "message-19"}},
        },
    }
    written = api.s.put(api.base + path, json=value, timeout=10)
    assert written.status_code == 200, written.text
    restored = api.s.get(api.base + path, timeout=10)
    assert restored.status_code == 200, restored.text
    assert restored.json() == value
    stale = api.s.put(api.base + path, json={**value, "revision": 6}, timeout=10)
    assert stale.status_code == 409, stale.text
    restored = api.s.get(api.base + path, timeout=10)
    assert restored.json() == value
