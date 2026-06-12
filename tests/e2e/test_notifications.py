"""Notification endpoint tests."""

from helpers import CodeTermAPI, unique_name


def test_create_notification(api_only: CodeTermAPI):
    """Create a notification and verify it appears in the list."""
    title = unique_name("notif")
    result = api_only.post("/api/notifications", json={"title": title, "body": "test body"})
    assert "id" in result

    notifs = api_only.get("/api/notifications")
    assert isinstance(notifs, list)
    titles = [n["title"] for n in notifs]
    assert title in titles

    # Cleanup
    api_only.post("/api/notifications/clear")


def test_notification_levels(api_only: CodeTermAPI):
    """Create notifications with different levels: info, warning, error."""
    created_ids = []
    for level in ("info", "warning", "error"):
        title = unique_name(f"notif-{level}")
        result = api_only.post(
            "/api/notifications",
            json={"title": title, "level": level},
        )
        assert "id" in result
        created_ids.append(result["id"])

    notifs = api_only.get("/api/notifications")
    found_levels = {n["level"].lower() for n in notifs if n["id"] in created_ids}
    assert "info" in found_levels
    assert "warning" in found_levels
    assert "error" in found_levels

    api_only.post("/api/notifications/clear")


def test_clear_notifications(api_only: CodeTermAPI):
    """Create then clear all notifications."""
    api_only.post("/api/notifications", json={"title": "to-clear"})
    result = api_only.post("/api/notifications/clear")
    assert result.get("ok") is True

    notifs = api_only.get("/api/notifications")
    assert len(notifs) == 0
