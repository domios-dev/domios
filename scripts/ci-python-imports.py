import ast
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]


def external_test_imports(root=ROOT):
    modules = set()
    for filename in (root / "scripts/tests").glob("*.py"):
        if not (filename.name.startswith("test_") or filename.name.endswith("_test.py")):
            continue
        for node in ast.walk(ast.parse(filename.read_text(encoding="utf-8-sig"))):
            names = [alias.name for alias in node.names] if isinstance(node, ast.Import) else (
                [node.module] if isinstance(node, ast.ImportFrom) and node.module and not node.level else []
            )
            for name in names:
                top = name.split(".")[0]
                local = any((directory / top).is_dir() or (directory / (top + ".py")).is_file()
                            for directory in [root, root / "scripts", root / "scripts/tests"])
                if top not in sys.stdlib_module_names and not local:
                    modules.add(top)
    return sorted(modules)


if __name__ == "__main__":
    print(json.dumps(external_test_imports()))
