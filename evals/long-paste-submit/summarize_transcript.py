import pathlib
import sys


text = pathlib.Path(sys.argv[1]).read_text(encoding="utf-8", errors="replace")
for token in sys.argv[2:]:
    print(f"transcript_contains[{token}]={str(token in text).lower()}")
