"""Supporter API contract against the authenticated dev daemon."""


def test_supporter_features_route_removed(api_only):
    before = api_only.get("/api/supporter/status")
    response = api_only.s.post(
        f"{api_only.base}/api/supporter/features",
        json={"memory_rerank": False, "tab_state": False},
        timeout=10,
    )
    assert response.status_code == 404
    after = api_only.get("/api/supporter/status")
    assert after["adapter"] == before["adapter"]
    assert after["model"] == before["model"]
    assert after["connected"] == before["connected"]


def test_supporter_status_activity_budget_contract(api_only):
    status = api_only.get("/api/supporter/status")
    assert isinstance(status["connected"], bool)
    assert isinstance(status["calls_last_hour"], int)
    assert isinstance(status["calls_last_day"], int)
    assert status["calls_last_day"] >= status["calls_last_hour"]
    assert status["last_activity"] is None or isinstance(status["last_activity"], int)
    assert "features" not in status
    assert set(status["last_verdict"]).issubset({"rerank", "tab_state"})
    budget = status["budget"]
    assert isinstance(budget["reached"], bool)
    assert isinstance(budget["calls_per_hour"], int)
    if budget["reached"]:
        assert isinstance(budget["free_at"], int)
    if status["connected"]:
        expected = "credit_exhausted" if status["spend"]["exhausted"] else "budget_reached" if budget["reached"] else "active"
        assert status["state"] == expected
    spend = status["spend"]
    for period in ("today", "month"):
        assert isinstance(spend[period]["input_tokens"], int)
        assert isinstance(spend[period]["output_tokens"], int)
        assert spend[period]["estimated_usd"] is None or isinstance(spend[period]["estimated_usd"], (int, float))
    assert isinstance(spend["exhausted"], bool)


def test_supporter_status_has_no_owner_entered_balance_contract(api_only):
    status = api_only.get("/api/supporter/status")
    assert set(status["spend"]) == {"today", "month", "exhausted", "next_probe_at", "storage_ready"}
    response = api_only.s.post(
        f"{api_only.base}/api/supporter/balance",
        json={},
        timeout=10,
    )
    assert response.status_code == 404
