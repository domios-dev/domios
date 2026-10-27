import json
import re
import subprocess
import time
from pathlib import Path

from release_issue_contract import (
    ContractError, PUBLIC_REPOSITORY, SOURCE_REPOSITORY,
    boundary_run_id, closure_boundary_refusal, resolve_sources,
)


MAX_BOUNDARIES = 50
MAX_COMMITS = 10000
MAX_REMOTE_CALLS = 1000
MAX_PR_PAGES = 10


def included_history(version, source_sha, candidates, read_commits, request):
    if not re.fullmatch(r"[0-9a-f]{40}", source_sha):
        raise ContractError("included history requires an exact source SHA")
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise ContractError("included history requires a numeric release version")
    current = tuple(map(int, version.split(".")))
    ordered = {}
    for candidate in candidates:
        value = candidate["version"]
        if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", value):
            continue
        if not re.fullmatch(r"[0-9a-f]{40}", candidate["sha"]):
            raise ContractError("release boundary requires an exact source SHA")
        key = tuple(map(int, value.split(".")))
        if key >= current:
            continue
        if key in ordered and ordered[key]["sha"] != candidate["sha"]:
            raise ContractError("conflicting reachable release tags for " + value)
        ordered[key] = candidate
    calls = 0
    cache = {}

    def remote(route, raw=False):
        nonlocal calls
        key = (route, raw)
        if key not in cache:
            if calls >= MAX_REMOTE_CALLS:
                raise ContractError("included history exceeds remote-call bound")
            calls += 1
            cache[key] = request(route, raw)
        return cache[key]

    start = None
    inspected = []
    for index, key in enumerate(sorted(ordered, reverse=True)):
        if index >= MAX_BOUNDARIES:
            raise ContractError("no verified boundary within candidate bound")
        candidate = ordered[key]
        tag = "v" + candidate["version"]
        private = remote(f"repos/{SOURCE_REPOSITORY}/releases/tags/{tag}")
        public = remote(f"repos/{PUBLIC_REPOSITORY}/releases/tags/{tag}")
        row = {**candidate, "private_published": False, "public_published": False}
        for release, field in [(private, "private_published"), (public, "public_published")]:
            row[field] = bool(release and not release.get("draft") and release.get("published_at"))
        private_assets = {asset["name"]: asset for asset in (private or {}).get("assets", [])}
        public_assets = {asset["name"] for asset in (public or {}).get("assets", [])}
        if (row["private_published"] and row["public_published"]
                and "issue-closure-boundary.json" in private_assets
                and {"issue-closure-boundary.json", "domios-latest.json"} <= public_assets):
            asset = private_assets["issue-closure-boundary.json"]
            boundary = remote(f"repos/{SOURCE_REPOSITORY}/releases/assets/{asset['id']}", True)
            try:
                row["closure_boundary"] = json.loads(boundary)
            except (ValueError, TypeError):
                row["closure_boundary"] = None
            run_id = boundary_run_id(row["closure_boundary"])
            if run_id:
                row["validator_run"] = remote(f"repos/{SOURCE_REPOSITORY}/actions/runs/{run_id}")
        row["refusal"] = closure_boundary_refusal(row)
        inspected.append(row)
        if row["private_published"] and row["public_published"] and row["refusal"] is None:
            start = candidate["sha"]
            break

    commits = read_commits(start, source_sha, MAX_COMMITS + 1)
    if len(commits) > MAX_COMMITS:
        raise ContractError("included commit range exceeds bound")
    included = {commit["sha"] for commit in commits}
    prs = {}
    for commit in commits:
        for page in range(1, MAX_PR_PAGES + 1):
            batch = remote(f"repos/{SOURCE_REPOSITORY}/commits/{commit['sha']}/pulls?per_page=100&page={page}")
            for pr in batch:
                if not pr.get("merged_at") or pr.get("merge_commit_sha") not in included:
                    continue
                body = pr.get("body") or ""
                if not re.search(r"(?:Public-Issue:|Repository-Issue:)", body, re.I) and not (
                        pr["merge_commit_sha"] == source_sha and re.search(
                            r"^(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?)\s+", body, re.I | re.M)):
                    continue
                prs[pr["number"]] = {"number": pr["number"], "url": pr["html_url"],
                                     "body": body, "base_ref": pr["base"]["ref"]}
            if len(batch) < 100:
                break
        else:
            raise ContractError("commit PR inventory exceeds page bound")
    selected_prs = sorted(prs.values(), key=lambda pr: pr["number"])
    resolved = resolve_sources(commits, selected_prs)
    states = {}
    for issue in resolved["issues"]:
        number = issue["reference"].rsplit("#", 1)[1]
        state = remote(f"repos/{PUBLIC_REPOSITORY}/issues/{number}")
        if not state or state.get("state") not in {"open", "closed"}:
            raise ContractError("public issue state is unreadable: " + issue["reference"])
        states[issue["reference"]] = state["state"].upper()
    return {"start": start, "source_sha": source_sha, "history": inspected,
            "commits": commits, "prs": selected_prs, "remote_calls": calls,
            "resolved": resolved, "states": states}


def git_output(args):
    return subprocess.check_output(["git", *args], text=True, encoding="utf-8", timeout=120)


def reachable_candidates(source_sha):
    tags = git_output(["tag", "--merged", source_sha, "--list", "release-*", "v*"]).splitlines()
    selected = [tag for tag in tags if re.fullmatch(r"(?:release-|v)[0-9]+\.[0-9]+\.[0-9]+", tag)]
    if not selected:
        return []
    objects = git_output(["rev-parse", *[tag + "^{commit}" for tag in selected]]).splitlines()
    return [{"version": re.sub(r"^(?:release-|v)", "", tag), "sha": sha}
            for tag, sha in zip(selected, objects, strict=True)]


def read_git_commits(start, source_sha, limit):
    revision = f"{start}..{source_sha}" if start else source_sha
    fields = git_output(["log", f"--max-count={limit}", "--format=%H%x00%B%x00", revision]).split("\0")
    return [{"sha": fields[i].strip(), "body": fields[i + 1],
             "url": f"https://github.com/{SOURCE_REPOSITORY}/commit/{fields[i].strip()}"}
            for i in range(0, len(fields) - 1, 2)]


def github_request(route, raw=False, timeout=60):
    args = ["gh", "api", route]
    if raw:
        args += ["-H", "Accept: application/octet-stream"]
    result = subprocess.run(args, capture_output=True, text=True, encoding="utf-8", timeout=timeout)
    if result.returncode:
        if "HTTP 404" in result.stderr:
            return None
        raise ContractError("GitHub request failed: " + route + "\n" + result.stderr)
    return result.stdout if raw else json.loads(result.stdout)


def resolve_included(version, source_sha, output_dir):
    deadline = time.monotonic() + 600

    def request(route, raw=False):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise ContractError("included history exhausted its remote-work budget")
        return github_request(route, raw, timeout=min(60, remaining))

    result = included_history(version, source_sha, reachable_candidates(source_sha), read_git_commits, request)
    directory = Path(output_dir)
    directory.mkdir(parents=True, exist_ok=True)
    for name, value in [("release-history", result["history"]), ("release-commits", result["commits"]),
                        ("release-prs", result["prs"]), ("resolved-public-issues", result["resolved"]),
                        ("public-issue-states", result["states"]),
                        ("issue-resolution-evidence", result)]:
        (directory / (name + ".json")).write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
    return result
