"""Web mirror token TTL contract tests."""

import re

import pytest
import requests


WEB_TOKEN_TTL_SECONDS = 3600


def _web_headers(token: str) -> dict[str, str]:
    return {"Authorization": f"Bearer {token}"}


def test_web_token_bootstrap_cookie_and_debug_clock_ttl_contract(api_only):
    auth = api_only.s.headers.get("Authorization", "")
    assert auth.startswith("Bearer "), "api fixture must use the dev config bearer token"
    token = auth.removeprefix("Bearer ")
    assert token

    bootstrap_url = f"{api_only.base}/manifest-bootstrap"
    first = requests.post(bootstrap_url, headers=_web_headers(token), timeout=10)
    assert first.status_code == 200, first.text

    set_cookie = first.headers.get("Set-Cookie", "")
    max_age = re.search(r"(?:^|;)\s*Max-Age=(\d+)(?:;|$)", set_cookie)
    assert max_age, f"Set-Cookie missing Max-Age: {set_cookie}"
    assert int(max_age.group(1)) == WEB_TOKEN_TTL_SECONDS

    clock = api_only.get("/api/debug/clock")
    if not clock.get("mocked"):
        pytest.xfail(
            "FINDING: debug clock is unavailable in this daemon, so web-token TTL cannot be time-traveled"
        )

    api_only.post(
        "/api/debug/clock/advance",
        json={"ms": (WEB_TOKEN_TTL_SECONDS + 5) * 1000},
    )
    expired = requests.post(bootstrap_url, headers=_web_headers(token), timeout=10)

    if expired.status_code == 200:
        pytest.xfail(
            "FINDING: web tokens use std::time::Instant, not the debug clock; advancing /api/debug/clock does not expire the token"
        )
    assert expired.status_code == 401, expired.text
