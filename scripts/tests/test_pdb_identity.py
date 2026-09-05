"""The symbol-store key a retained PDB is filed under.

`build.sh` files a dev build's symbols under this key and `crash::report` writes the
same key into the sidecar's `Symbols:` line. If the two ever disagree, a crash names
a directory that retention never wrote — which is the 2026-09-05 failure.
"""

import importlib.util
import struct
import sys
from pathlib import Path

MODULE = Path(__file__).resolve().parents[1] / "pdb_identity.py"
spec = importlib.util.spec_from_file_location("pdb_identity", MODULE)
pdb_identity = importlib.util.module_from_spec(spec)
sys.modules["pdb_identity"] = pdb_identity
spec.loader.exec_module(pdb_identity)

BLOCK = 512

# {06BBF58E-ADAC-4DCD-AE38-084A16853450} — the build that aborted at 12:36:11Z.
GUID_BYTES = bytes(
    [0x8E, 0xF5, 0xBB, 0x06]        # Data1, little-endian
    + [0xAC, 0xAD]                  # Data2
    + [0xCD, 0x4D]                  # Data3
    + [0xAE, 0x38, 0x08, 0x4A, 0x16, 0x85, 0x34, 0x50]
)
EXPECTED_KEY = "06BBF58EADAC4DCDAE38084A168534501"


def write_pdb(path: Path, guid: bytes, age: int) -> None:
    """A minimal but structurally real MSF 7.0 container carrying a PDB info stream."""
    info = struct.pack("<III", 20000404, 0, age) + guid          # version, signature, age, guid
    directory = struct.pack("<III", 2, 0, len(info)) + struct.pack("<I", 6)
    blocks = [bytes(BLOCK) for _ in range(7)]
    blocks[0] = (
        pdb_identity.MSF_MAGIC
        + struct.pack("<IIIIII", BLOCK, 1, 7, len(directory), 0, 3)
    ).ljust(BLOCK, b"\0")
    blocks[3] = struct.pack("<I", 4).ljust(BLOCK, b"\0")          # block map -> directory block
    blocks[4] = directory.ljust(BLOCK, b"\0")
    blocks[6] = info.ljust(BLOCK, b"\0")
    path.write_bytes(b"".join(blocks))


def test_key_matches_the_symbol_store_form(tmp_path):
    pdb = tmp_path / "codeterm.pdb"
    write_pdb(pdb, GUID_BYTES, 1)

    assert pdb_identity.pdb_identity(pdb) == ("06BBF58E-ADAC-4DCD-AE38-084A16853450", 1)
    assert pdb_identity.identity_key(pdb_identity.pdb_identity(pdb)) == EXPECTED_KEY


def test_key_carries_no_separators_and_ends_with_the_age(tmp_path):
    pdb = tmp_path / "codeterm.pdb"
    write_pdb(pdb, GUID_BYTES, 17)
    key = pdb_identity.identity_key(pdb_identity.pdb_identity(pdb))
    assert "-" not in key
    assert key == EXPECTED_KEY[:-1] + "11"  # 17 == 0x11


def test_two_builds_never_share_a_key(tmp_path):
    first, second = tmp_path / "a.pdb", tmp_path / "b.pdb"
    write_pdb(first, GUID_BYTES, 1)
    write_pdb(second, bytes([GUID_BYTES[0] ^ 0xFF]) + GUID_BYTES[1:], 1)
    assert pdb_identity.pdb_identity(first) != pdb_identity.pdb_identity(second)


def test_a_pe_rsds_record_yields_the_same_key_shape():
    record = b"RSDS" + GUID_BYTES + struct.pack("<I", 1) + b"codeterm.pdb\0"
    assert pdb_identity.identity_key(pdb_identity.cv_identity(record)) == EXPECTED_KEY
    assert pdb_identity.describe_cv(record) == (
        "06BBF58E-ADAC-4DCD-AE38-084A16853450 age=1 pdb=codeterm.pdb"
    )


def test_a_file_that_is_neither_pdb_nor_pe_has_no_identity(tmp_path):
    plain = tmp_path / "notes.txt"
    plain.write_text("not a symbol file")
    assert pdb_identity.identity_of(plain) is None
    assert pdb_identity.identity_key(None) is None
