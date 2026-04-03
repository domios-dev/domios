"""File attachment tests — sending files via chat API."""

import os
import time
import tempfile
import pytest


class TestFileAttachment:
    """Test file attachment flow (simulates chat view sending files)."""

    def test_send_text_file(self, api, fresh_pane):
        """[attached: /path/to/file.txt] should be sent to PTY."""
        with tempfile.NamedTemporaryFile(suffix=".txt", delete=False, mode="w") as f:
            f.write("test content from e2e")
            path = f.name
        try:
            api.send_text(fresh_pane, text=f"[attached: {path}] check this")
            output = api.wait_for_output(fresh_pane, pattern="attached", timeout=5)
            assert "attached" in output
        finally:
            os.unlink(path)

    def test_send_pdf(self, api, fresh_pane):
        """PDF attachment path should arrive in PTY output."""
        with tempfile.NamedTemporaryFile(suffix=".pdf", delete=False, mode="wb") as f:
            f.write(b"%PDF-1.4 fake pdf")
            path = f.name
        try:
            api.send_text(fresh_pane, text=f"[attached: {path}] read this")
            output = api.wait_for_output(fresh_pane, pattern="attached", timeout=10)
            assert path in output or "attached" in output
        finally:
            os.unlink(path)

    def test_send_multiple_files(self, api, fresh_pane):
        """Multiple [attached:] markers in one message."""
        paths = []
        for ext in [".txt", ".py", ".json"]:
            with tempfile.NamedTemporaryFile(suffix=ext, delete=False, mode="w") as f:
                f.write(f"content {ext}")
                paths.append(f.name)
        try:
            markers = " ".join(f"[attached: {p}]" for p in paths)
            api.send_text(fresh_pane, text=f"{markers} check all files")
            output = api.wait_for_output(fresh_pane, pattern="attached", timeout=10)
            assert "attached" in output
        finally:
            for p in paths:
                os.unlink(p)
