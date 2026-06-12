"""Logging system tests."""

from pathlib import Path
import pytest

LOG_PATH = Path.home() / ".codeterm-dev" / "logs" / "codeterm.log"


def _log_text() -> str:
    return LOG_PATH.read_text(encoding="utf-8", errors="replace")


class TestLogging:
    """Verify file logging system works."""

    def test_log_file_exists(self):
        assert LOG_PATH.exists(), f"Log file not found at {LOG_PATH}"

    def test_log_has_startup_message(self):
        if not LOG_PATH.exists():
            pytest.skip("No log file")
        assert "CodeTerm starting" in _log_text()

    def test_log_has_info_level(self):
        if not LOG_PATH.exists():
            pytest.skip("No log file")
        assert " INFO " in _log_text()

    def test_log_has_timestamps(self):
        if not LOG_PATH.exists():
            pytest.skip("No log file")
        # Format: [2026-03-28T08:00:00Z INFO ...]
        content = _log_text()
        assert "[20" in content, "Log should have ISO timestamps"
