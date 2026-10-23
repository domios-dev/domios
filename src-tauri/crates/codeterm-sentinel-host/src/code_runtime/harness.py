import contextlib
import importlib.util
import json
import os
import sys

root = os.path.dirname(__file__)
incoming = json.load(sys.stdin)
with open(os.path.join(root, "output.json"), encoding="utf-8") as stream:
    output = json.load(stream)
spec = importlib.util.spec_from_file_location("domios_body", os.path.join(root, "body.py"))
body = importlib.util.module_from_spec(spec)
with contextlib.redirect_stdout(sys.stderr):
    spec.loader.exec_module(body)
    if not callable(getattr(body, "main", None)):
        os.write(2, b"DOMIOS_CONTRACT_VIOLATION\n")
        raise SystemExit(78)
    value = body.main(incoming["value"])
output["value"] = value
output["text"] = ""
json.dump(output, sys.stdout, separators=(",", ":"), ensure_ascii=False, allow_nan=False)
