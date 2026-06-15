"""Health and identify endpoint tests."""

from helpers import CodeTermAPI


def test_health(api_only: CodeTermAPI):
    """GET /api/health returns status ok."""
    data = api_only.get("/api/health")
    assert data["status"] == "ok"
    assert "version" in data
    assert data["name"] == "CodeTerm"


def test_identify(api_only: CodeTermAPI):
    """GET /api/identify returns version, platform, features."""
    data = api_only.get("/api/identify")
    assert data["name"] == "CodeTerm"
    assert "version" in data
    assert "platform" in data
    assert "features" in data
    assert isinstance(data["features"], list)
