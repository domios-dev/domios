import argparse
import base64
import json
import pathlib
import re


parser = argparse.ArgumentParser()
parser.add_argument("cast")
parser.add_argument("--log")
parser.add_argument("--log-offset", type=int, default=0)
parser.add_argument("--pty")
parser.add_argument("--expected-source-bytes", type=int)
args = parser.parse_args()

input_events = []
output_events = []
with pathlib.Path(args.cast).open(encoding="utf-8") as stream:
    header = json.loads(next(stream))
    for raw in stream:
        row = json.loads(raw)
        if len(row) >= 3 and row[1] == "i":
            event = row[2]
            data = base64.b64decode(event["bytes_b64"])
            input_events.append((float(row[0]), event["source"], data))
        elif len(row) >= 3 and row[1] == "o":
            output_events.append((float(row[0]), row[2]))
        elif len(row) >= 3 and row[1] == "o+b64":
            output_events.append(
                (float(row[0]), base64.b64decode(row[2]).decode("utf-8", errors="replace"))
            )

print(
    f"cast_geometry={header['width']}x{header['height']} "
    f"input_frames={len(input_events)} output_frames={len(output_events)}"
)
previous_time = None
payload_indices = []
for index, (timestamp, source, data) in enumerate(input_events, start=1):
    gap = "first" if previous_time is None else f"gap_ms={(timestamp - previous_time) * 1000:.1f}"
    if data == b"\r":
        kind = "submit_cr"
        detail = "hex=0d separate_frame=true"
    elif data.startswith(b"\x1b[200~") and data.endswith(b"\x1b[201~"):
        content = data[len(b"\x1b[200~") : -len(b"\x1b[201~")]
        kind = "bracketed_paste"
        detail = f"payload_bytes={len(content)} closed=true"
        payload_indices.append(index - 1)
    else:
        kind = "raw_input"
        detail = f"bytes={len(data)} preview={data[:40]!r}"
        payload_indices.append(index - 1)
    print(f"input[{index}] t={timestamp:.6f} source={source} kind={kind} {detail} {gap}")
    previous_time = timestamp

for payload_number, event_index in enumerate(payload_indices, start=1):
    next_index = (
        payload_indices[payload_number]
        if payload_number < len(payload_indices)
        else len(input_events)
    )
    submit_count = sum(
        1 for _, _, data in input_events[event_index + 1 : next_index] if data == b"\r"
    )
    print(f"payload[{payload_number}] submit_cr_before_next_payload={submit_count}")

placeholder = re.compile(r"\[Pasted Content \d+ chars(?: #\d+)?\]")
paste_time = next(
    (timestamp for timestamp, _, data in input_events if data.startswith(b"\x1b[200~")),
    None,
)
rendered = ""
placeholder_event = None
for timestamp, text in output_events:
    rendered = (rendered + text)[-2_000_000:]
    if paste_time is None or timestamp < paste_time:
        continue
    for match in placeholder.finditer(rendered):
        count = int(re.search(r"\d+", match.group(0)).group(0))
        expected = args.expected_source_bytes
        if expected is None or abs(count - expected) <= 128:
            placeholder_event = (timestamp, match.group(0))
            break
    if placeholder_event:
        break
if placeholder_event:
    delta = "unknown" if paste_time is None else f"{(placeholder_event[0] - paste_time) * 1000:.1f}"
    print(
        f"collapsed_paste_render t={placeholder_event[0]:.6f} "
        f"after_paste_ms={delta} text={placeholder_event[1]!r}"
    )
else:
    print(
        "collapsed_paste_render not_observed "
        f"expected_source_bytes={args.expected_source_bytes}"
    )

if args.log:
    raw = pathlib.Path(args.log).read_bytes()[args.log_offset :]
    text = raw.decode("utf-8", errors="replace")
    print("audit_events:")
    for line in text.splitlines():
        if args.pty and f"pty={args.pty}" not in line and args.pty not in line:
            continue
        if any(
            tag in line
            for tag in (
                "[submit-cycle]",
                "[submit-audit]",
                "[submit-recovery]",
                "[idle-poll]",
                "[status]",
                "[chat-send]",
            )
        ):
            print(line)
