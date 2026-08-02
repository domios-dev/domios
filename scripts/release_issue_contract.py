#!/usr/bin/env python3
import argparse
import json
import re
import sys
from pathlib import Path


PUBLIC_REPOSITORY = "rollacode/codeterm-releases"
REFERENCE = re.compile(r"^Public-Issue: rollacode/codeterm-releases#([1-9][0-9]*)$")
REFERENCE_MARKER = re.compile(r"Public-Issue:", re.IGNORECASE)
CLOSING_KEYWORD = re.compile(
    r"\b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?)\s+"
    r"(?:[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)?#[0-9]+\b",
    re.IGNORECASE,
)
ISSUE_LIKE = re.compile(r"(?<![A-Za-z0-9_])(?:[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)?#[0-9]+\b")


class ContractError(ValueError):
    pass


def parse_body(body: str) -> list[str]:
    references: list[str] = []
    seen: set[str] = set()
    errors: list[str] = []
    for line_number, line in enumerate(body.splitlines(), 1):
        match = REFERENCE.fullmatch(line)
        if match:
            reference = f"{PUBLIC_REPOSITORY}#{match.group(1)}"
            if reference in seen:
                errors.append(f"line {line_number}: duplicate {reference}")
            else:
                seen.add(reference)
                references.append(reference)
            continue
        if REFERENCE_MARKER.search(line):
            errors.append(f"line {line_number}: malformed Public-Issue metadata")
        elif CLOSING_KEYWORD.search(line):
            errors.append(f"line {line_number}: GitHub closing keywords are forbidden")
        elif ISSUE_LIKE.search(line):
            errors.append(f"line {line_number}: issue references must use Public-Issue metadata")
    if errors:
        raise ContractError("\n".join(errors))
    return sorted(references, key=lambda value: int(value.rsplit("#", 1)[1]))


def resolve_prs(prs: list[dict]) -> dict:
    issues: dict[str, list[dict]] = {}
    for pr in prs:
        number = pr.get("number")
        url = pr.get("url")
        body = pr.get("body") or ""
        if not isinstance(number, int) or not isinstance(url, str):
            raise ContractError("each PR requires integer number and string url")
        try:
            references = parse_body(body)
        except ContractError as error:
            raise ContractError(f"PR #{number}: {error}") from error
        for reference in references:
            issues.setdefault(reference, []).append({"number": number, "url": url})
    return {
        "repository": PUBLIC_REPOSITORY,
        "issues": [
            {"reference": reference, "prs": issues[reference]}
            for reference in sorted(issues, key=lambda value: int(value.rsplit("#", 1)[1]))
        ],
    }


def plan_close(resolved: dict, states: dict, version: str) -> list[dict]:
    actions = []
    for issue in resolved.get("issues", []):
        reference = issue["reference"]
        if states.get(reference) != "OPEN":
            continue
        number = int(reference.rsplit("#", 1)[1])
        for pr in issue["prs"]:
            actions.append(
                {
                    "number": number,
                    "reference": reference,
                    "comment": f"Released in v{version}\n\n{pr['url']}",
                }
            )
            break
    return actions


def read_json(path: str):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def main() -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    parse_parser = subparsers.add_parser("parse")
    parse_parser.add_argument("--body-file", required=True)
    resolve_parser = subparsers.add_parser("resolve")
    resolve_parser.add_argument("--prs-file", required=True)
    plan_parser = subparsers.add_parser("plan-close")
    plan_parser.add_argument("--resolved-file", required=True)
    plan_parser.add_argument("--states-file", required=True)
    plan_parser.add_argument("--version", required=True)
    args = parser.parse_args()
    try:
        if args.command == "parse":
            for reference in parse_body(Path(args.body_file).read_text(encoding="utf-8")):
                print(reference)
        elif args.command == "resolve":
            print(json.dumps(resolve_prs(read_json(args.prs_file)), indent=2))
        else:
            print(json.dumps(plan_close(read_json(args.resolved_file), read_json(args.states_file), args.version), indent=2))
    except (ContractError, OSError, json.JSONDecodeError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
