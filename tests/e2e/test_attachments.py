"""File attachment tests — sending files via chat API."""

import os
import hashlib
import time
import tempfile
import pytest

from helpers import unique_name


def _identity_command(path: str, proof: str) -> str:
    return (
        "python -c \"import pathlib,hashlib; "
        f"p=r'{path}'; "
        "b=pathlib.Path(p).read_bytes(); "
        f"print('{proof}_%d_%s' % (len(b), hashlib.sha256(b).hexdigest()[:16]))\""
    )


class TestFileAttachment:
    """Test file attachment flow (simulates chat view sending files)."""

    def test_send_text_file(self, api, fresh_pane):
        """[attached: /path/to/file.txt] should be sent to PTY."""
        with tempfile.NamedTemporaryFile(suffix=".txt", delete=False, mode="w") as f:
            content = "test content from e2e"
            f.write(content)
            path = f.name
        digest = hashlib.sha256(content.encode()).hexdigest()[:16]
        proof = unique_name("ATTACH_TXT")
        try:
            expected = f"{proof}_{len(content.encode())}_{digest}"
            api.send_text(fresh_pane, text=f"{_identity_command(path, proof)} # [attached: {path}]")
            output = api.wait_for_output(fresh_pane, pattern=expected, timeout=10)
            assert expected in output
        finally:
            os.unlink(path)

    def test_send_pdf(self, api, fresh_pane):
        """PDF attachment path should arrive in PTY output."""
        with tempfile.NamedTemporaryFile(suffix=".pdf", delete=False, mode="wb") as f:
            content = b"%PDF-1.4 fake pdf"
            f.write(content)
            path = f.name
        digest = hashlib.sha256(content).hexdigest()[:16]
        proof = unique_name("ATTACH_PDF")
        try:
            expected = f"{proof}_{len(content)}_{digest}"
            api.send_text(fresh_pane, text=f"{_identity_command(path, proof)} # [attached: {path}]")
            output = api.wait_for_output(fresh_pane, pattern=expected, timeout=10)
            assert expected in output
        finally:
            os.unlink(path)

    def test_send_multiple_files(self, api, fresh_pane):
        """Multiple [attached:] markers in one message."""
        paths = []
        expected = []
        for ext in [".txt", ".py", ".json"]:
            with tempfile.NamedTemporaryFile(suffix=ext, delete=False, mode="w") as f:
                content = f"content {ext}"
                f.write(content)
                paths.append(f.name)
                digest = hashlib.sha256(content.encode()).hexdigest()[:16]
                expected.append((f.name, unique_name(f"ATTACH_{ext[1:].upper()}"), len(content.encode()), digest))
        try:
            commands = [
                _identity_command(path, proof)
                for path, proof, _, _ in expected
            ]
            api.send_text(fresh_pane, text="; ".join(commands))
            last = f"{expected[-1][1]}_{expected[-1][2]}_{expected[-1][3]}"
            output = api.wait_for_output(fresh_pane, pattern=last, timeout=15)
            for _, proof, size, digest in expected:
                assert f"{proof}_{size}_{digest}" in output
        finally:
            for p in paths:
                os.unlink(p)
