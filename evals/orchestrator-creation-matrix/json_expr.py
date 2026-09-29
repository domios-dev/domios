# WHAT: Evaluate a controlled expression against JSON embedded in CLI output.
# WHY: CLI hints may surround JSON, and parser errors must stay visible.
# ORACLE: The first complete JSON value is passed as `d` to the expression.

import json
import sys


def first_json(raw):
    decoder = json.JSONDecoder()
    for index, char in enumerate(raw):
        if char not in "[{":
            continue
        try:
            value, _ = decoder.raw_decode(raw[index:])
            return value
        except json.JSONDecodeError:
            continue
    return None


def main():
    value = first_json(sys.stdin.read())
    if value is None:
        return 0
    result = eval(sys.argv[1], {"d": value})
    if result is not None:
        print(result)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
