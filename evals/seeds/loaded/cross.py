"""Auto-split persona seeds (was inline in loaded.py).

Use _add() to append to this module's ITEMS — each persona keeps its own list,
which the loaded package concatenates in __init__.py.
"""

ITEMS: list[dict] = []


def _add(**kw):
    ITEMS.append(kw)


# ─────────────────────────────────────────────────────────────
# AUTOMATION (personal bots / scheduled tasks) — ~20 items
# ─────────────────────────────────────────────────────────────

_add(kind="skill", title="Send Telegram message via bot",
    body="curl -X POST https://api.telegram.org/bot$TOKEN/sendMessage -d chat_id=$CHAT -d text='...'. Token in keychain as telegram-bot-token.",
    area="automation", importance=4, tags=["telegram"])

_add(kind="skill", title="Home Assistant notify service",
    body="From HA: service notify.mobile_app_phone, data { message: '...', title: '...' }. From outside: POST /api/services/notify/mobile_app_phone with bearer token.",
    area="automation", importance=4, tags=["homeassistant", "notify"])

_add(kind="fact", title="Telegram webhook endpoint",
    body="Telegram bot webhook points to server-mini:8443 via tailscale funnel. Cloudflared tunnel terminates TLS. Certificate auto-renewed.",
    area="automation", importance=3, tags=["telegram"])

_add(kind="learning", title="HA automation debounce",
    body="Motion-triggered light automation fires multiple times if motion is intermittent. Added a debounce condition: state hasn't changed in the last 30s.",
    area="automation", importance=3, tags=["homeassistant"])

_add(kind="skill", title="Add a new scheduled task",
    body="Edit ~/infra/launchd/com.user.<task>.plist, load with launchctl load -w <plist>. Logs in ~/Library/Logs/<task>.log.",
    area="automation", importance=3, tags=["launchd"])

_add(kind="note", title="Morning routine automation",
    body="At 7:30am on weekdays: turn on bedroom lights gradually, play a wake-up playlist via Home Assistant, set coffee maker via smart plug.",
    area="automation", importance=2, tags=["routine"])

_add(kind="fact", title="MQTT broker config",
    body="Mosquitto runs on server-mini, port 1883 internal, 8883 TLS for external. Clients auth with username/password from HA secrets file.",
    area="automation", importance=3, tags=["mqtt"])

_add(kind="learning", title="Webhook retry strategy",
    body="Telegram bot webhook should respond 200 within 30s or Telegram retries. For long-running commands, ack immediately, process async, send message as reply.",
    area="automation", importance=4, tags=["telegram"])

_add(kind="rule", title="No secrets in HA yaml",
    body="HA configuration yaml must reference secrets.yaml for tokens. Never paste API keys directly in config files. secrets.yaml is gitignored.",
    area="automation", importance=5, tags=["security", "rule"])

_add(kind="preference", title="Notification tone",
    body="For automation notifications: prefer factual over cute. 'Motion detected in garage, 3:14am' not 'Hey! Something's happening!'.",
    area="automation", importance=2, tags=["style"])

_add(kind="skill", title="Telegram command handler",
    body="Parse /commands in bot update handler. Command args follow after space. Prefix unknown commands with error message, don't crash.",
    area="automation", importance=3, tags=["telegram"])

# ─────────────────────────────────────────────────────────────
# PERSONAL preferences + identity — ~15 items
# ─────────────────────────────────────────────────────────────

_add(kind="preference", title="Editor",
    body="Primary editor is VSCode with vim keybindings. Secondary is vim for quick edits. Font: JetBrains Mono 14pt. Theme: Dracula.",
    area="personal", importance=3, tags=["editor"])

_add(kind="preference", title="Terminal theme",
    body="Terminal uses Dracula color scheme. Background slightly transparent (90%). Font: JetBrains Mono 13pt.",
    area="personal", importance=2, tags=["terminal"])

_add(kind="preference", title="Response style",
    body="Prefer concise responses. No padding, no 'great question', no 'happy to help'. Assume technical baseline. Sharp direct feedback.",
    area="personal", importance=5, tags=["communication"])

_add(kind="preference", title="Language",
    body="Code, comments, and docs in English only. Chat replies follow user's language. Thinking/reasoning always English for token efficiency.",
    area="personal", importance=5, tags=["communication"])

_add(kind="rule", title="Never commit without explicit request",
    body="Do not create git commits unless user explicitly asks. No auto-commits at the end of tasks. Let the user review and decide.",
    area="personal", importance=5, tags=["git", "rule"])

_add(kind="rule", title="Don't add unnecessary comments",
    body="Default to no comments. Only add when the WHY is non-obvious. Never comment what the code does — naming should convey that.",
    area="personal", importance=4, tags=["style", "rule"])

_add(kind="preference", title="Task style",
    body="When given a task, dive in and do it. Ask only if truly ambiguous or destructive. Report findings at the end, not a running commentary.",
    area="personal", importance=4, tags=["workflow"])

_add(kind="preference", title="Test before reporting done",
    body="Run relevant unit tests, type checks, or builds before declaring a task complete. Blocked/partial is acceptable, silent broken is not.",
    area="personal", importance=4, tags=["workflow"])

_add(kind="rule", title="No emoji in code or docs",
    body="Don't add emojis to code, comments, commit messages, or documentation files unless explicitly requested. Emojis acceptable in chat replies only when the user uses them.",
    area="personal", importance=3, tags=["style", "rule"])

_add(kind="preference", title="Commit message format",
    body="Conventional commits style: <type>(<scope>): <subject>. Body explains why, not what. No emojis. No Co-Authored-By unless pair programming.",
    area="personal", importance=3, tags=["git"])

# ─────────────────────────────────────────────────────────────
# Daily / summary items (simulate past dream diaries) — ~10 items
# ─────────────────────────────────────────────────────────────

_add(kind="daily", title="Dream diary 2026-04-04",
    body="Today: worked on Beacon Postgres slow query fix. Talked to Jordan about Django 5 upgrade. Consolidated 3 duplicate PgBouncer facts into canonical config item.",
    area=None, importance=2, tags=["dream-diary"])

_add(kind="daily", title="Dream diary 2026-04-05",
    body="Mostly Aurora work. Alex proposed mixer refactor. Morgan shared onboarding mockups. Fixed StoreKit transaction finishing bug. 4 new learnings added.",
    area=None, importance=2, tags=["dream-diary"])

_add(kind="daily", title="Dream diary 2026-04-06",
    body="Cobalt dashboard work with Riley. Playwright flake investigation. Discussed RSC and decided to postpone. 2 duplicate React Query notes merged.",
    area=None, importance=2, tags=["dream-diary"])

_add(kind="daily", title="Dream diary 2026-04-07",
    body="Infra day. Kai upgraded Home Assistant. Fixed docker disk usage. Added launchd task for nightly backups. No new duplicates.",
    area=None, importance=2, tags=["dream-diary"])

_add(kind="summary", title="Q1 themes",
    body="Q1 dominant themes: Beacon transcription reliability (Deepgram→Whisper fallback), Aurora Observation framework migration, Cobalt bundle size reduction. 3 major postmortems, 2 regressions fixed.",
    area=None, importance=3, tags=["summary", "quarter"])

_add(kind="summary", title="Persistent concerns",
    body="Recurring pain points across projects: Celery task idempotency (Beacon), Audio callback thread safety (Aurora), Okta session expiry UX (Cobalt). Each has open tickets and work ongoing.",
    area=None, importance=3, tags=["summary"])

# ─────────────────────────────────────────────────────────────
# A few scripts (short code snippets as skills)
# ─────────────────────────────────────────────────────────────

_add(kind="script", title="Quick Postgres slow query finder",
    body="SELECT query, calls, mean_exec_time FROM pg_stat_statements ORDER BY mean_exec_time DESC LIMIT 20; — requires pg_stat_statements extension enabled.",
    area="beacon", importance=3, tags=["postgres", "sql"])

_add(kind="script", title="Docker cleanup one-liner",
    body="docker system prune -af --volumes — removes all unused images, containers, networks, volumes. Use with caution, no confirmation.",
    area="infra", importance=3, tags=["docker"])

_add(kind="script", title="Kill process on port",
    body="lsof -ti :<port> | xargs kill -9 — kills whatever is listening on that port. On macOS and Linux. For Windows use netstat -ano findstr :<port>.",
    area="infra", importance=3, tags=["unix"])

# ─────────────────────────────────────────────────────────────
# EXTRA infra + automation
# ─────────────────────────────────────────────────────────────

_add(kind="learning", title="Tailscale funnel certificate",
    body="Tailscale Funnel provisions a Let's Encrypt cert automatically but only for *.tailnet-name.ts.net domains. Custom domains need a separate reverse proxy.",
    area="infra", importance=3, tags=["tailscale"])

_add(kind="learning", title="systemd-like launchd daemons",
    body="launchd KeepAlive true auto-restarts the daemon on crash, but only after a throttle interval (10s). SuccessfulExit false is often what you actually want.",
    area="infra", importance=3, tags=["launchd"])

_add(kind="skill", title="Inspect docker network",
    body="docker network inspect <name> shows connected containers, subnet, gateway. docker network prune removes all unused networks.",
    area="infra", importance=2, tags=["docker"])

_add(kind="note", title="Kai proposed mesh monitoring",
    body="Kai wants to set up Prometheus on server-mini to scrape metrics from all tailnet machines. Needs node_exporter on each. Planned for next month.",
    area="infra", importance=2, tags=["monitoring"])

_add(kind="fact", title="DNS setup",
    body="Internal DNS via tailscale MagicDNS. No manual Pi-hole or coredns. Lookup format: <machine>.<tailnet>.ts.net. Works across OS.",
    area="infra", importance=3, tags=["dns"])

_add(kind="learning", title="docker-compose healthcheck gotcha",
    body="depends_on: service_healthy only works if the depended-on service has a healthcheck defined. Without it, depends_on is just a start-order hint.",
    area="infra", importance=3, tags=["docker"])

_add(kind="skill", title="Diagnose high CPU on server-mini",
    body="ssh server-mini && top -o cpu. Or htop if installed. For per-container: docker stats --no-stream. Usually the culprit is a runaway HA automation.",
    area="infra", importance=3, tags=["ops"])

_add(kind="note", title="Backup restore drill",
    body="Quarterly drill: restore HA backup to a VM, verify automations and integrations load. Last drill: all clean except one broken template sensor (manual fix).",
    area="infra", importance=3, tags=["backup", "drill"])

_add(kind="fact", title="Mesh bandwidth budget",
    body="Tailnet peak bandwidth is ~50Mbps (upload-bound by home ISP). Heavy sync tasks (ML datasets) scheduled for 3am-6am off-peak.",
    area="infra", importance=2, tags=["mesh"])

# ─────────────────────────────────────────────────────────────
# Extra notes to fill out
# ─────────────────────────────────────────────────────────────

_add(kind="note", title="Weekend: tried Nix on mac",
    body="Spent Saturday trying to set up Nix for reproducible dev environments. Got a basic flake working. Too much overhead for personal use, parked the experiment.",
    area="personal", importance=1, tags=["nix", "experiment"])

_add(kind="note", title="Read paper on speculative decoding",
    body="Read recent paper on speculative decoding for LLM inference. Technique can ~2x decode speed by running a small draft model alongside the big one. Interesting for Beacon.",
    area="personal", importance=2, tags=["ml", "reading"])

_add(kind="learning", title="macOS SIP and system modifications",
    body="macOS System Integrity Protection (SIP) blocks writes to /System and /usr. Legitimate system mods require boot to Recovery and csrutil disable. Don't do it.",
    area="infra", importance=3, tags=["macos"])

_add(kind="skill", title="Generate SSH key pair",
    body="ssh-keygen -t ed25519 -C 'comment' -f ~/.ssh/keyname. Add to ssh-agent: ssh-add ~/.ssh/keyname. Copy public to remote ~/.ssh/authorized_keys.",
    area="infra", importance=3, tags=["ssh"])

_add(kind="fact", title="Dotfiles repo",
    body="All dotfiles in ~/dotfiles, symlinked via stow. Zsh config, git config, VSCode settings, tmux conf. Private keys not included.",
    area="personal", importance=2, tags=["dotfiles"])

_add(kind="skill", title="Profile Python memory",
    body="memory_profiler + @profile decorator shows line-by-line memory usage. tracemalloc for snapshot comparison. Useful for finding leaks in long-running workers.",
    area="beacon", importance=2, tags=["python", "perf"])

_add(kind="rule", title="Always pin Docker base images",
    body="Never use :latest in production Dockerfiles. Pin to specific tags or digests. Reproducibility > convenience.",
    area="infra", importance=4, tags=["docker", "rule"])

_add(kind="preference", title="Reading list format",
    body="Keep a list of papers/articles to read in a plain text file. One line per item, prefix with [x] when done. No app, no database, just text.",
    area="personal", importance=1, tags=["reading"])

_add(kind="note", title="Experimented with local models",
    body="Ran Llama 3.1 70B on desktop-gpu via ollama. Usable for chat, struggles with long context. Keeps memory for follow-ups better than expected.",
    area="personal", importance=2, tags=["llm", "experiment"])
