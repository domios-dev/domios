#!/usr/bin/env python3
import argparse
import json
import re
import sys
from pathlib import Path


PUBLIC_REPOSITORY = "rollacode/codeterm-releases"
SOURCE_REPOSITORY = "rollacode/codeterm"
# A declaration may close a prose line: commit messages are immutable, so a
# trailing marker must resolve instead of failing every range that contains it.
PUBLIC_REFERENCE = re.compile(
    r"(?:^|(?<=\s))Public-Issue: rollacode/codeterm-releases#([1-9][0-9]*)$"
)
REPOSITORY_REFERENCE = re.compile(
    r"(?:^|(?<=\s))Repository-Issue: rollacode/codeterm#([1-9][0-9]*)$"
)
INLINE_CODE_SPAN = re.compile(r"`[^`]*`")
PUBLIC_REFERENCE_MARKER = re.compile(r"Public-Issue:", re.IGNORECASE)
REPOSITORY_REFERENCE_MARKER = re.compile(r"Repository-Issue:", re.IGNORECASE)
REPOSITORY_CLOSE = re.compile(r"^Closes #([1-9][0-9]*)$")
CLOSING_KEYWORD = re.compile(
    r"\b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?)\s+"
    r"(?:(?:[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)?#[0-9]+|"
    r"https?://(?:www\.)?github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+/issues/[0-9]+)\b",
    re.IGNORECASE,
)
# GitHub honours the full issue URL as a closing keyword too, so an audit that
# only reads the #N forms would pass a commit closing an issue by link.
CLOSING_KEYWORD_TARGET = re.compile(
    r"\b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?)\s+"
    r"(?:"
    r"(?P<repository>[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)?#(?P<number>[0-9]+)"
    r"|https?://(?:www\.)?github\.com/"
    r"(?P<url_repository>[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)/issues/(?P<url_number>[0-9]+)"
    r")\b",
    re.IGNORECASE,
)
ISSUE_LIKE = re.compile(r"(?<![A-Za-z0-9_])(?:[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+)?#[0-9]+\b")
PUBLIC_ISSUE_URL = re.compile(
    r"https?://(?:www\.)?github\.com/rollacode/codeterm-releases/issues/[0-9]+\b",
    re.IGNORECASE,
)


class ContractError(ValueError):
    pass


def parse_metadata(
    body: str,
    *,
    reject_incidental_references: bool = True,
    allow_repository_closes: bool = False,
) -> tuple[list[str], list[str]]:
    public_references: list[str] = []
    repository_references: list[str] = []
    seen: set[str] = set()
    errors: list[str] = []
    for line_number, line in enumerate(body.splitlines(), 1):
        public_match = PUBLIC_REFERENCE.search(line)
        repository_match = REPOSITORY_REFERENCE.search(line)
        repository_close = REPOSITORY_CLOSE.fullmatch(line) if allow_repository_closes else None
        declaration = public_match or repository_match or repository_close
        if declaration:
            repository = PUBLIC_REPOSITORY if public_match else SOURCE_REPOSITORY
            reference = f"{repository}#{declaration.group(1)}"
            if reference in seen:
                errors.append(f"line {line_number}: duplicate {reference}")
            else:
                seen.add(reference)
                target = public_references if public_match else repository_references
                target.append(reference)
        # Text before a trailing declaration is still prose, held to the same rules.
        remainder = line[: declaration.start()] if declaration else line
        # A backticked span quotes the syntax rather than using it. Without this a
        # commit that documents the contract fails the contract, permanently: a
        # message on master cannot be edited, so every later release re-reads it.
        remainder = INLINE_CODE_SPAN.sub(" ", remainder)
        if PUBLIC_REFERENCE_MARKER.search(remainder):
            errors.append(f"line {line_number}: malformed Public-Issue metadata")
        elif REPOSITORY_REFERENCE_MARKER.search(remainder):
            errors.append(f"line {line_number}: malformed Repository-Issue metadata")
        elif reject_incidental_references and CLOSING_KEYWORD.search(remainder):
            errors.append(f"line {line_number}: GitHub closing keywords are forbidden")
        elif reject_incidental_references and (
            ISSUE_LIKE.search(remainder) or PUBLIC_ISSUE_URL.search(remainder)
        ):
            errors.append(f"line {line_number}: issue references must use issue metadata")
    if errors:
        raise ContractError("\n".join(errors))
    def reference_number(value: str) -> int:
        return int(value.rsplit("#", 1)[1])

    return (
        sorted(public_references, key=reference_number),
        sorted(repository_references, key=reference_number),
    )


def parse_body(body: str, *, reject_incidental_references: bool = True) -> list[str]:
    return parse_metadata(body, reject_incidental_references=reject_incidental_references)[0]


def repository_closes(body: str) -> list[str]:
    return [
        f"Closes #{reference.rsplit('#', 1)[1]}"
        for reference in parse_metadata(body)[1]
    ]


def _reference_numbers(references: list[str]) -> set[int]:
    return {int(reference.rsplit("#", 1)[1]) for reference in references}


def reconcile(body: str, commits: list[dict], prs: list[dict]) -> list[str]:
    """The rendered closing block is the only source of the closing set.

    A body is not evidence about itself: it is checked against the range that
    actually merged, so an issue nobody fixed cannot be closed by writing its
    number down, and an issue that was fixed cannot be dropped by forgetting it.
    """
    declared = parse_metadata(body)[1]
    resolved = [
        issue["reference"]
        for issue in resolve_sources(commits, prs)["repository_issues"]
    ]
    unfixed = sorted(set(declared) - set(resolved), key=lambda value: int(value.rsplit("#", 1)[1]))
    undeclared = sorted(set(resolved) - set(declared), key=lambda value: int(value.rsplit("#", 1)[1]))
    errors = []
    if unfixed:
        errors.append(
            "body declares issues no merged commit or PR fixed: " + ", ".join(unfixed)
        )
    if undeclared:
        errors.append(
            "merged range fixed issues the body does not declare: " + ", ".join(undeclared)
        )
    if errors:
        raise ContractError("\n".join(errors))
    return repository_closes(body)


def audit_commits(commits: list[dict], body: str) -> None:
    """A closing keyword is a promise the merge will keep. Auditing only that the
    promises are present would pass on a range that closes the wrong issues, so
    both directions are checked: no keyword outside the fixed set, and no fixed
    issue left unclaimed."""
    fixed = _reference_numbers(parse_metadata(body)[1])
    claimed: set[int] = set()
    errors: list[str] = []
    for commit in commits:
        sha = commit.get("sha")
        if not isinstance(sha, str) or not re.fullmatch(r"[0-9a-f]{40}", sha):
            raise ContractError("each commit requires a full lowercase sha")
        for match in CLOSING_KEYWORD_TARGET.finditer(commit.get("body") or ""):
            repository = match.group("repository") or match.group("url_repository")
            number = match.group("number") or match.group("url_number")
            if repository and repository != SOURCE_REPOSITORY:
                errors.append(
                    f"commit {sha}: closing keyword for {repository}#{number}; "
                    "public release issues close after verified publication, never on merge"
                )
                continue
            value = int(number)
            if value not in fixed:
                errors.append(
                    f"commit {sha}: closing keyword names {SOURCE_REPOSITORY}#{value}, "
                    "which this range did not fix"
                )
                continue
            claimed.add(value)
    unclaimed = sorted(fixed - claimed)
    if unclaimed:
        errors.append(
            "fixed issues no commit claims with a closing keyword: "
            + ", ".join(f"{SOURCE_REPOSITORY}#{number}" for number in unclaimed)
        )
    if errors:
        raise ContractError("\n".join(errors))


def resolve_sources(commits: list[dict], prs: list[dict]) -> dict:
    public_issues: dict[str, list[dict]] = {}
    repository_issues: dict[str, list[dict]] = {}

    def add_sources(target: dict[str, list[dict]], references: list[str], source: dict) -> None:
        for reference in references:
            target.setdefault(reference, []).append(source)

    for pr in prs:
        number = pr.get("number")
        url = pr.get("url")
        body = pr.get("body") or ""
        base_ref = pr.get("base_ref")
        if not isinstance(number, int) or not isinstance(url, str):
            raise ContractError("each PR requires integer number and string url")
        if base_ref is not None and not isinstance(base_ref, str):
            raise ContractError("PR base_ref must be a string when present")
        try:
            public_references, repository_references = parse_metadata(
                body,
                allow_repository_closes=base_ref == "master",
            )
        except ContractError as error:
            raise ContractError(f"PR #{number}: {error}") from error
        source = {"kind": "pull_request", "number": number, "url": url}
        add_sources(public_issues, public_references, source)
        add_sources(repository_issues, repository_references, source)
    for commit in commits:
        sha = commit.get("sha")
        url = commit.get("url")
        body = commit.get("body") or ""
        if not isinstance(sha, str) or not re.fullmatch(r"[0-9a-f]{40}", sha):
            raise ContractError("each commit requires a full lowercase sha")
        if not isinstance(url, str):
            raise ContractError("each commit requires a string url")
        try:
            public_references, repository_references = parse_metadata(
                body, reject_incidental_references=False
            )
        except ContractError as error:
            raise ContractError(f"commit {sha}: {error}") from error
        source = {"kind": "commit", "sha": sha, "url": url}
        add_sources(public_issues, public_references, source)
        add_sources(repository_issues, repository_references, source)

    def resolved_issues(issues: dict[str, list[dict]]) -> list[dict]:
        for sources in issues.values():
            unique = {source["url"]: source for source in sources}
            sources[:] = sorted(
                unique.values(),
                key=lambda source: (source["kind"] != "pull_request", source["url"]),
            )
        return [
            {"reference": reference, "sources": issues[reference]}
            for reference in sorted(issues, key=lambda value: int(value.rsplit("#", 1)[1]))
        ]

    return {
        "repository": PUBLIC_REPOSITORY,
        "issues": resolved_issues(public_issues),
        "source_repository": SOURCE_REPOSITORY,
        "repository_issues": resolved_issues(repository_issues),
    }


def plan_close(resolved: dict, states: dict, version: str) -> list[dict]:
    actions = []
    for issue in resolved.get("issues", []):
        reference = issue["reference"]
        if states.get(reference) != "OPEN":
            continue
        number = int(reference.rsplit("#", 1)[1])
        for source in issue["sources"]:
            actions.append(
                {
                    "number": number,
                    "reference": reference,
                    "comment": f"Released in v{version}\n\n{source['url']}",
                }
            )
            break
    return actions


def select_range_start(releases: list[dict]) -> str | None:
    verified = [
        release
        for release in releases
        if release.get("private_published")
        and release.get("public_published")
        and release.get("mirror_verified")
    ]
    if not verified:
        return None
    try:
        selected = max(
            verified,
            key=lambda release: tuple(int(part) for part in release["version"].split(".")),
        )
    except (KeyError, AttributeError, ValueError) as error:
        raise ContractError("release history requires numeric X.Y.Z versions") from error
    sha = selected.get("sha")
    if not isinstance(sha, str) or not re.fullmatch(r"[0-9a-f]{40}", sha):
        raise ContractError("verified release requires a full commit sha")
    return sha


def read_json(path: str):
    return json.loads(Path(path).read_text(encoding="utf-8"))


def main() -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)
    parse_parser = subparsers.add_parser("parse")
    parse_parser.add_argument("--body-file", required=True)
    closes_parser = subparsers.add_parser("repository-closes")
    closes_parser.add_argument("--body-file", required=True)
    resolve_parser = subparsers.add_parser("resolve")
    resolve_parser.add_argument("--commits-file", required=True)
    resolve_parser.add_argument("--prs-file", required=True)
    plan_parser = subparsers.add_parser("plan-close")
    plan_parser.add_argument("--resolved-file", required=True)
    plan_parser.add_argument("--states-file", required=True)
    plan_parser.add_argument("--version", required=True)
    reconcile_parser = subparsers.add_parser("reconcile")
    reconcile_parser.add_argument("--body-file", required=True)
    reconcile_parser.add_argument("--commits-file", required=True)
    reconcile_parser.add_argument("--prs-file", required=True)
    audit_parser = subparsers.add_parser("audit-commits")
    audit_parser.add_argument("--commits-file", required=True)
    audit_parser.add_argument("--body-file", required=True)
    range_parser = subparsers.add_parser("range-start")
    range_parser.add_argument("--releases-file", required=True)
    args = parser.parse_args()
    try:
        if args.command == "parse":
            for reference in parse_body(Path(args.body_file).read_text(encoding="utf-8")):
                print(reference)
        elif args.command == "repository-closes":
            for line in repository_closes(Path(args.body_file).read_text(encoding="utf-8")):
                print(line)
        elif args.command == "reconcile":
            for line in reconcile(
                Path(args.body_file).read_text(encoding="utf-8"),
                read_json(args.commits_file),
                read_json(args.prs_file),
            ):
                print(line)
        elif args.command == "audit-commits":
            audit_commits(
                read_json(args.commits_file),
                Path(args.body_file).read_text(encoding="utf-8"),
            )
        elif args.command == "resolve":
            print(json.dumps(resolve_sources(read_json(args.commits_file), read_json(args.prs_file)), indent=2))
        elif args.command == "plan-close":
            print(json.dumps(plan_close(read_json(args.resolved_file), read_json(args.states_file), args.version), indent=2))
        else:
            start = select_range_start(read_json(args.releases_file))
            if start:
                print(start)
    except (ContractError, OSError, json.JSONDecodeError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
