import contextlib
import errno
import importlib.util
import json
import os
import sys

root = os.path.dirname(__file__)
permission_denied = False


def observe(frame, event, arg):
    global permission_denied
    if event == "exception":
        error = arg[1]
        if isinstance(error, PermissionError) and error.errno in (errno.EPERM, errno.EACCES):
            permission_denied = True
            os.write(2, b"DOMIOS_SANDBOX_DENIED\n")
    return observe


incoming = json.load(sys.stdin)
with open(os.path.join(root, "output.json"), encoding="utf-8") as stream:
    output = json.load(stream)
spec = importlib.util.spec_from_file_location("domios_body", os.path.join(root, "body.py"))
body = importlib.util.module_from_spec(spec)
sys.settrace(observe)
try:
    with contextlib.redirect_stdout(sys.stderr):
        spec.loader.exec_module(body)
        if not callable(getattr(body, "main", None)):
            os.write(2, b"DOMIOS_CONTRACT_VIOLATION\n")
            raise SystemExit(78)
        value = body.main(incoming["value"])
finally:
    sys.settrace(None)
if permission_denied:
    raise SystemExit(77)
output["value"] = value
output["text"] = ""
json.dump(output, sys.stdout, separators=(",", ":"), ensure_ascii=False, allow_nan=False)
