"""Paste _kit.css between the /*KIT*/ and /*END KIT*/ markers of every artboard."""
import pathlib
import re

here = pathlib.Path(__file__).parent
kit = (here / "_kit.css").read_text(encoding="utf-8").strip()
block = re.compile(r"/\*KIT\*/.*?/\*END KIT\*/", re.S)

for board in sorted(here.glob("*.dc.html")):
    source = board.read_text(encoding="utf-8")
    pasted, count = block.subn(lambda _: f"/*KIT*/\n{kit}\n/*END KIT*/", source)
    if count != 1:
        raise SystemExit(f"{board.name}: expected one KIT block, found {count}")
    board.write_text(pasted, encoding="utf-8", newline="\n")
    print(f"pasted kit into {board.name}")
