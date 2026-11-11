"""Notification admission and independent level/severity fields."""

import pytest

from helpers import CodeTermAPI, unique_name


LIVE_CALLER_REQUIRED = "Bell notifications require notify send from a live agent caller tab"


def _create_unattributed(api, **fields):
    title = unique_name("notif")
    response = api.s.post(
        f"{api.base}/api/notifications",
        json={"title": title, "body": "test body", **fields},
        timeout=10,
    )
    return title, response


def test_create_notification_requires_live_caller(api_only: CodeTermAPI):
    title, response = _create_unattributed(api_only)
    assert response.status_code == 400, response.text
    assert response.text == LIVE_CALLER_REQUIRED
    assert all(n["title"] != title for n in api_only.get("/api/notifications"))


@pytest.mark.parametrize("level", ["verbose", "action_needed", "failure"])
@pytest.mark.parametrize("severity", ["info", "warning", "error"])
def test_notification_levels_and_severities_require_live_caller(api_only, level, severity):
    _, response = _create_unattributed(api_only, level=level, severity=severity)
    assert response.status_code == 400, response.text
    assert response.text == LIVE_CALLER_REQUIRED


@pytest.mark.parametrize("level", ["info", "warning", "error", "unknown"])
def test_notification_rejects_invalid_level(api_only, level):
    _, response = _create_unattributed(api_only, level=level, severity="info")
    assert response.status_code == 400, response.text
    assert "Unknown notification level" in response.text
    for supported in ("verbose", "action_needed", "failure"):
        assert supported in response.text


def test_notification_severity_requires_string(api_only):
    _, response = _create_unattributed(api_only, level="verbose", severity=123)
    assert response.status_code == 422, response.text
    assert "severity" in response.text


def test_clear_notifications(api_only: CodeTermAPI):
    result = api_only.post("/api/notifications/clear")
    assert result["ok"] is True
    assert api_only.get("/api/notifications") == []
