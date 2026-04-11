"""
Seed dataset for the "loaded" fixture — ~200 anonymized mem items.

All names/projects/people are fictional. Technologies are real (PgBouncer,
SwiftUI, Docker, etc.) because they're public and needed for realism.

Personas:
  project_aurora   — cross-platform audio wellness app (iOS-focused)
  project_beacon   — AI meeting assistant (Python backend, transcription)
  project_cobalt   — internal enterprise platform (React frontend)
  project_delta    — home-lab infra / mesh
  project_echo     — CLI tool being built (meta)

  person_alex      — aurora lead dev (iOS)
  person_morgan    — aurora designer
  person_sam       — aurora audio engineer
  person_jordan    — beacon backend
  person_taylor    — beacon ML/transcription
  person_riley     — cobalt fullstack
  person_kai       — delta infra

Areas: aurora, beacon, cobalt, infra, automation, echo, personal

The file exports ITEMS (list of dicts). Run `python load.py` to POST them.
Intentional near-duplicates and supersede pairs are marked with comments.
"""

ITEMS: list[dict] = []


def add(**kw):
    ITEMS.append(kw)


# ─────────────────────────────────────────────────────────────
# AURORA (wellness audio iOS app) — ~40 items
# ─────────────────────────────────────────────────────────────

add(kind="fact", title="Aurora audio engine",
    body="Aurora uses Core Audio AVAudioEngine with a custom mixer node. SwiftUI for UI, UIKit for the player view controller. Minimum iOS 16.",
    area="aurora", importance=4, tags=["audio", "architecture"])

add(kind="fact", title="Aurora HealthKit integration",
    body="Aurora writes sleep session data to HealthKit with category type sleepAnalysis. Asks authorization on first launch of sleep module.",
    area="aurora", importance=4, tags=["healthkit", "sleep"])

add(kind="fact", title="Aurora StoreKit 2 subscriptions",
    body="Uses StoreKit 2 AsyncSequence for transaction updates. Two tiers: monthly and yearly. Free tier limited to 3 sessions per day.",
    area="aurora", importance=4, tags=["storekit", "iap"])

add(kind="fact", title="Aurora minimum iOS version",
    body="Minimum deployment target is iOS 16.0. Dropped iOS 15 support in Q4 to use Observation framework.",
    area="aurora", importance=3, tags=["ios", "deployment"])

# NEAR-DUPLICATE of "Aurora audio engine" above — different wording
add(kind="fact", title="Aurora tech stack",
    body="Aurora is built on Core Audio (AVAudioEngine) for sound pipeline, SwiftUI for main UI, and UIKit only for the custom player screen. Targets iOS 16+.",
    area="aurora", importance=3, tags=["architecture"])

add(kind="learning", title="AVAudioEngine background interruption",
    body="When backgrounded during a phone call, AVAudioEngine loses its connection graph. Must reconnect nodes in notification handler or audio dies silently.",
    area="aurora", importance=5, tags=["audio", "gotcha"])

add(kind="learning", title="HealthKit sleep write permissions",
    body="HealthKit requires HKSharePermissionRequestType to write sleep categories. Read-only permission is NOT enough even for updating existing samples we created.",
    area="aurora", importance=4, tags=["healthkit", "gotcha"])

add(kind="learning", title="StoreKit 2 transaction finishing",
    body="Must call Transaction.finish() after validating purchase server-side. Otherwise the transaction stays in the queue forever and triggers on every launch.",
    area="aurora", importance=5, tags=["storekit", "gotcha"])

add(kind="learning", title="Core Audio sample rate mismatch",
    body="If input and output nodes have different sample rates, AVAudioEngine will crash on start. Insert a converter node between them or match rates explicitly.",
    area="aurora", importance=4, tags=["audio"])

add(kind="note", title="Talked to Alex about mixer refactor",
    body="Alex wants to split the mixer into separate AudioUnit instances so we can add per-layer effects. Agreed to prototype on a branch next week.",
    area="aurora", importance=2, tags=["meeting"])

add(kind="note", title="Morgan onboarding redesign",
    body="Morgan showed new onboarding mockups. Moving from 4-screen carousel to single scrollable flow. Uses new illustration set.",
    area="aurora", importance=2, tags=["design"])

add(kind="note", title="Sam tuning reverb preset",
    body="Sam found that reverb wet mix at 0.35 sounds best for the rain preset. Dry preset uses 0.15. Values committed to presets.json.",
    area="aurora", importance=2, tags=["audio"])

add(kind="skill", title="How to run Aurora tests locally",
    body="cd apps/aurora && xcodebuild test -scheme Aurora -destination 'platform=iOS Simulator,name=iPhone 15'. Needs Xcode 15+.",
    area="aurora", importance=3, tags=["testing"])

add(kind="skill", title="Aurora TestFlight release",
    body="Bump version in Config.xcconfig, archive via CI (push to release branch), auto-uploads to TestFlight. Approval takes ~20 minutes.",
    area="aurora", importance=3, tags=["release"])

add(kind="rule", title="Never block main thread in audio callback",
    body="Audio callback runs on a realtime thread. No locks, no malloc, no Objective-C dispatch. Use lock-free ring buffers only.",
    area="aurora", importance=5, tags=["audio", "rule"])

add(kind="preference", title="Aurora branching convention",
    body="Feature branches are feat/<short-desc>. Bugfix branches are fix/<short-desc>. Release branches are release/<version>.",
    area="aurora", importance=2, tags=["git"])

add(kind="fact", title="Aurora active user count",
    body="Aurora had 12k monthly actives at end of Q1. Growth is ~8% MoM. Churn is ~4% for paid tier.",
    area="aurora", importance=3, tags=["metrics"])

add(kind="fact", title="Aurora crash rate",
    body="Crash-free rate is 99.6% on iOS 17, 99.2% on iOS 16. Main crash source is the legacy UIKit player view controller.",
    area="aurora", importance=3, tags=["metrics", "crash"])

# SUPERSEDED pair — old and new deployment target
add(kind="fact", title="Aurora old deployment target",
    body="Aurora minimum iOS version used to be 15.0 during Q1-Q3 of last year. Bumped to 16.0 in Q4 when we adopted the Observation framework.",
    area="aurora", importance=1, tags=["ios", "history", "outdated"])

add(kind="fact", title="Aurora audio layer count",
    body="Aurora supports up to 4 simultaneous audio layers. Limit is CPU-bound on older iPhones (SE, 11-series).",
    area="aurora", importance=3, tags=["audio", "limits"])

add(kind="learning", title="SwiftUI List performance",
    body="SwiftUI List with 500+ items drops frames on iPhone SE. Switched to LazyVStack inside ScrollView, smoother but loses swipe actions.",
    area="aurora", importance=3, tags=["swiftui", "perf"])

add(kind="learning", title="Combine memory leak with shared subjects",
    body="Assigning a PassthroughSubject to a @Published property creates a retain cycle. Use .assign(to: &$property) with a weak closure instead.",
    area="aurora", importance=4, tags=["combine", "memory"])

add(kind="skill", title="Aurora localization workflow",
    body="Run fastlane localize — pulls new strings from Lokalise, merges into Localizable.strings per locale, commits. Currently 8 languages.",
    area="aurora", importance=2, tags=["i18n"])

add(kind="note", title="Aurora 2.1 release postmortem",
    body="2.1 had a regression: subscription status wasn't refreshing after purchase. Hotfix 2.1.1 released 3 days later. Root cause was missing .current check.",
    area="aurora", importance=3, tags=["release", "postmortem"])

add(kind="fact", title="Aurora subscription price points",
    body="Monthly $4.99, yearly $29.99 (50% savings). Lifetime one-time $79.99. Prices shown via StoreKit, localized automatically.",
    area="aurora", importance=3, tags=["storekit", "pricing"])

add(kind="conversation", title="Team sync about iPad support",
    body="Team discussed whether to invest in iPad-optimized layout. Decision: postpone to Q3 — not enough demand signal from analytics to justify the work.",
    area="aurora", importance=2, tags=["planning"])

add(kind="fact", title="Aurora CI provider",
    body="Aurora builds on GitHub Actions using macos-14 runners. Build+test+archive takes ~18 minutes. Fastlane for signing.",
    area="aurora", importance=3, tags=["ci"])


# ─────────────────────────────────────────────────────────────
# BEACON (AI meeting assistant, Python backend) — ~35 items
# ─────────────────────────────────────────────────────────────

add(kind="fact", title="Beacon PgBouncer config",
    body="Beacon uses PgBouncer in transaction pooling mode with max_client_conn=400. Sits in front of the primary Postgres. One bouncer per region.",
    area="beacon", importance=5, tags=["pgbouncer", "db"])

# NEAR-DUPLICATE (wording 2)
add(kind="fact", title="Beacon connection pooler",
    body="Connection pooling for Beacon's Postgres is done via PgBouncer running in transaction mode. Max client connections set to 400.",
    area="beacon", importance=3, tags=["pgbouncer"])

# NEAR-DUPLICATE (wording 3)
add(kind="fact", title="Beacon database pooling",
    body="Postgres connections for Beacon are routed through PgBouncer with transaction-level pooling. Configured for 400 concurrent clients.",
    area="beacon", importance=3, tags=["db", "pooling"])

add(kind="fact", title="Beacon stack overview",
    body="Beacon backend is Django 5 + Celery + Redis + Postgres. LangGraph for agent orchestration. WebRTC via LiveKit for real-time audio capture.",
    area="beacon", importance=5, tags=["architecture"])

add(kind="fact", title="Beacon transcription provider",
    body="Beacon uses Deepgram Nova-2 for primary transcription. Falls back to Whisper large-v3 via internal GPU worker if Deepgram rate-limits.",
    area="beacon", importance=4, tags=["transcription"])

add(kind="fact", title="Beacon auth provider",
    body="Beacon uses Auth0 for SSO. JWT tokens validated via JWKS endpoint. Refresh tokens stored httpOnly cookies.",
    area="beacon", importance=4, tags=["auth"])

add(kind="learning", title="Auth0 JWT kid header requirement",
    body="Auth0 requires the kid header in JWT even though RFC says it's optional. Without kid, JWKS lookup fails silently and every request comes back 401.",
    area="beacon", importance=5, tags=["auth", "jwt", "gotcha"])

# NEAR-DUPLICATE of the auth0 learning above
add(kind="learning", title="JWT refresh 401 fix",
    body="Auth0 token refresh was returning 401 until we added the kid header to JWT construction. Spec marks kid optional but Auth0 needs it.",
    area="beacon", importance=3, tags=["jwt", "fix"])

add(kind="learning", title="LangGraph state persistence",
    body="LangGraph checkpointer must use a persistent backend (Postgres, not in-memory) for multi-step agent flows. In-memory checkpointer loses state between worker restarts.",
    area="beacon", importance=4, tags=["langgraph"])

add(kind="learning", title="LiveKit room cleanup",
    body="LiveKit rooms must be explicitly deleted via API after the meeting ends. Auto-cleanup on empty-for-10min doesn't trigger if any participant reconnects briefly.",
    area="beacon", importance=3, tags=["livekit"])

add(kind="note", title="Jordan proposed moving to Django 5",
    body="Jordan wants to upgrade Django 4 → 5 for async views support. Estimated 2-3 days of work. Concern: some Celery tasks use sync db adapter.",
    area="beacon", importance=2, tags=["django"])

add(kind="note", title="Taylor improved Whisper latency",
    body="Taylor reduced Whisper inference latency by ~30% by switching to faster-whisper and enabling CUDA graphs. Baseline 2.1s → 1.45s for 30s audio.",
    area="beacon", importance=3, tags=["ml", "perf"])

add(kind="skill", title="Beacon local dev",
    body="docker compose up in beacon/ starts postgres, redis, pgbouncer, django, celery worker. Then python manage.py runserver. Django admin at /admin.",
    area="beacon", importance=4, tags=["dev"])

add(kind="skill", title="Beacon deploy to staging",
    body="Push to release/staging branch. GitHub Actions builds image, pushes to ECR, triggers ECS task deploy. Takes ~6 minutes end-to-end.",
    area="beacon", importance=3, tags=["deploy"])

add(kind="skill", title="How to tail Beacon Celery logs",
    body="ssh beacon-staging, then docker logs -f beacon_celery_1. Or via aws logs tail /ecs/beacon-celery --follow.",
    area="beacon", importance=2, tags=["ops"])

add(kind="rule", title="Never commit AWS keys",
    body="Any commit containing AWS_SECRET_ACCESS_KEY triggers a pre-commit hook block. If accidentally committed, rotate the key immediately — even if the repo is private.",
    area="beacon", importance=5, tags=["security"])

add(kind="rule", title="Celery tasks must be idempotent",
    body="All Celery tasks must be safe to retry. Use database constraints and external idempotency keys for side-effect calls. Acks_late + task_reject_on_worker_lost enabled.",
    area="beacon", importance=4, tags=["celery", "rule"])

add(kind="preference", title="Beacon Python style",
    body="Black + ruff + mypy strict. Line length 100. Type hints on every function. No implicit Optional. Imports grouped by isort.",
    area="beacon", importance=2, tags=["style"])

add(kind="fact", title="Beacon LangGraph agent count",
    body="Beacon's LangGraph pipeline has 7 nodes: ingest, diarize, transcribe, align, summarize, extract_actions, format_output. Runs in parallel where DAG allows.",
    area="beacon", importance=4, tags=["langgraph"])

add(kind="fact", title="Beacon Postgres size",
    body="Primary Postgres is db.r6g.xlarge with 500GB gp3. ~40GB used for user data, ~120GB for transcripts, rest is indexes and WAL.",
    area="beacon", importance=3, tags=["db"])

add(kind="learning", title="Deepgram websocket reconnect",
    body="Deepgram streaming WS disconnects silently after ~3min of silence. Must send a keepalive frame every 60s or reconnect on the next audio chunk.",
    area="beacon", importance=4, tags=["deepgram", "gotcha"])

add(kind="learning", title="Redis OOM from transcript cache",
    body="Transcript intermediate cache with no TTL filled Redis in ~8 hours of usage. Set TTL 2h on the cache keys, added maxmemory-policy allkeys-lru as safety net.",
    area="beacon", importance=4, tags=["redis"])

# SUPERSEDED pair
add(kind="fact", title="Beacon old transcription provider",
    body="Before Deepgram, Beacon used AssemblyAI for transcription. Switched last quarter due to cost and worse accuracy on accented speech.",
    area="beacon", importance=1, tags=["transcription", "history", "outdated"])

add(kind="note", title="Jordan reported Postgres slow query",
    body="Jordan flagged a slow query on the meetings list endpoint — takes 8s with >1000 meetings. Needs index on (user_id, created_at DESC).",
    area="beacon", importance=3, tags=["db", "perf"])

add(kind="skill", title="Beacon DB migration",
    body="python manage.py makemigrations, then migrate. On prod: always run with --plan first to preview. Never edit historical migration files.",
    area="beacon", importance=3, tags=["db", "migration"])

add(kind="conversation", title="Team discussion about meeting length limit",
    body="Current limit is 4 hours. Team wants to raise to 8h. Concern: transcript memory grows O(n) and summarization model context window is the hard cap.",
    area="beacon", importance=2, tags=["planning"])

add(kind="fact", title="Beacon rate limit per user",
    body="Free tier: 10 meetings per month. Pro: unlimited meetings, 4h each. Enterprise: unlimited with custom length limit.",
    area="beacon", importance=3, tags=["pricing", "limits"])

add(kind="learning", title="Celery beat scheduler on redeploy",
    body="Celery beat loses its schedule on container restart unless using django-celery-beat with DB persistence. Otherwise scheduled tasks get double-run during rolling deploys.",
    area="beacon", importance=4, tags=["celery"])

add(kind="fact", title="Beacon regional deployment",
    body="Beacon runs in us-east-1 primary, eu-west-1 replica. DB replication lag typically <200ms. Failover is manual via Route53 weight change.",
    area="beacon", importance=3, tags=["infra"])


# ─────────────────────────────────────────────────────────────
# COBALT (internal React enterprise platform) — ~25 items
# ─────────────────────────────────────────────────────────────

add(kind="fact", title="Cobalt stack",
    body="Cobalt frontend is React 18 + TypeScript + Vite + TanStack Query + Zustand. Tailwind for styling. Playwright for E2E tests.",
    area="cobalt", importance=4, tags=["react", "architecture"])

add(kind="fact", title="Cobalt auth flow",
    body="Cobalt uses SAML SSO against Okta. Session cookie is httpOnly, SameSite=Lax, 12h expiry. No refresh token — full re-auth on expiry.",
    area="cobalt", importance=4, tags=["auth"])

add(kind="learning", title="TanStack Query stale while revalidate",
    body="TanStack Query refetches on window focus by default. Caused duplicate expensive API calls. Set refetchOnWindowFocus: false globally, opt-in per query.",
    area="cobalt", importance=3, tags=["react-query"])

add(kind="learning", title="Zustand persist middleware SSR",
    body="Zustand persist middleware reads localStorage synchronously in the initializer. Breaks SSR hydration. Wrap with typeof window check or use onRehydrateStorage.",
    area="cobalt", importance=3, tags=["zustand", "ssr"])

add(kind="note", title="Riley proposed dashboard redesign",
    body="Riley wants to split the main dashboard into panels that can be rearranged. Uses react-grid-layout. Mock in Figma approved by team.",
    area="cobalt", importance=2, tags=["design"])

add(kind="skill", title="Cobalt local dev",
    body="pnpm install && pnpm dev. Runs on http://localhost:5173. Mock API proxied to staging via vite.config.ts dev proxy.",
    area="cobalt", importance=3, tags=["dev"])

add(kind="skill", title="Cobalt Playwright tests",
    body="pnpm test:e2e runs Playwright against local dev server. CI runs against a preview deployment. Flaky tests auto-retry up to 2x.",
    area="cobalt", importance=2, tags=["testing"])

add(kind="rule", title="No untyped any in Cobalt",
    body="Cobalt eslint config forbids any. Use unknown + narrowing or proper generics. CI blocks PRs with any.",
    area="cobalt", importance=3, tags=["typescript", "rule"])

add(kind="preference", title="Cobalt commit style",
    body="Conventional commits: feat/fix/chore/docs/test/refactor. Scope is optional but encouraged for multi-module changes.",
    area="cobalt", importance=2, tags=["git"])

add(kind="fact", title="Cobalt bundle size",
    body="Cobalt production bundle is 420KB gzipped. Main chunk 180KB, vendor 240KB. Tracked via rollup-plugin-visualizer.",
    area="cobalt", importance=3, tags=["perf"])

add(kind="learning", title="Vite glob import performance",
    body="Using import.meta.glob('./routes/*.tsx', { eager: true }) loaded all 80 route modules on boot and tripled startup time. Switched to lazy glob + React.lazy.",
    area="cobalt", importance=3, tags=["vite", "perf"])

add(kind="learning", title="React 18 concurrent rendering pitfall",
    body="useEffect runs twice in StrictMode dev only. Side effects (analytics events, API calls with no cache key) must be idempotent or guarded with a ref.",
    area="cobalt", importance=3, tags=["react"])

add(kind="note", title="Playwright flake investigation",
    body="E2E login test fails ~5% runs. Root cause: Okta login iframe sometimes takes >3s to load. Bumped waitForSelector timeout to 15s.",
    area="cobalt", importance=2, tags=["testing"])

add(kind="fact", title="Cobalt deployment",
    body="Cobalt deploys to Cloudfront via S3. Main branch auto-deploys to staging, release branch to prod. CDN cache invalidated on deploy.",
    area="cobalt", importance=3, tags=["deploy"])

add(kind="skill", title="Cobalt CSS migration",
    body="When moving a component from CSS modules to Tailwind, use the codemod in scripts/to-tailwind.mjs. Handles ~80% of class mappings, rest manual.",
    area="cobalt", importance=2, tags=["css"])

add(kind="conversation", title="Riley asked about Server Components",
    body="Riley asked if Cobalt should adopt React Server Components. Team decided: not yet, our API boundary is clean and RSC adds complexity for no clear win.",
    area="cobalt", importance=2, tags=["planning"])

add(kind="fact", title="Cobalt test coverage",
    body="Unit test coverage is 68%. E2E covers the critical user flows (login, dashboard, 3 main CRUD pages). Target for Q2 is 80% unit.",
    area="cobalt", importance=2, tags=["testing"])


# ─────────────────────────────────────────────────────────────
# DELTA (home-lab infra / mesh) — ~25 items
# ─────────────────────────────────────────────────────────────

add(kind="peer", title="laptop-main",
    body="Primary dev laptop, macOS 15. Runs local dev servers, VSCode, Docker Desktop. On tailscale mesh as laptop-main.",
    area="infra", importance=4, tags=["machine"])

add(kind="peer", title="desktop-gpu",
    body="Desktop workstation with RTX 4090, Windows 11. Used for ML training and heavy CI. Reachable via tailscale as desktop-gpu.",
    area="infra", importance=4, tags=["machine"])

add(kind="peer", title="server-mini",
    body="Mac mini M2, always on, hosts Home Assistant, Mosquitto, internal bots, Docker services. Reachable as server-mini.",
    area="infra", importance=5, tags=["machine"])

add(kind="peer", title="mobile-dev",
    body="Secondary MacBook Pro, used for mobile builds and testing. On tailscale as mobile-dev. Xcode and Android Studio installed.",
    area="infra", importance=3, tags=["machine"])

add(kind="fact", title="Mesh topology",
    body="All machines are on a tailscale tailnet. server-mini acts as exit node for the home subnet. MagicDNS enabled, no manual IPs needed.",
    area="infra", importance=4, tags=["tailscale", "mesh"])

add(kind="fact", title="Home Assistant config location",
    body="Home Assistant lives on server-mini in docker compose. Config at /opt/homeassistant/config. Backed up nightly to S3 via rclone.",
    area="infra", importance=4, tags=["homeassistant"])

add(kind="fact", title="Tailscale ACL policy",
    body="Tailnet ACL restricts dev laptops from accessing the IoT subnet (192.168.10.0/24). Only server-mini can bridge. Policy in tailnet admin console.",
    area="infra", importance=4, tags=["tailscale", "security"])

add(kind="learning", title="Docker Desktop VM eating disk",
    body="Docker Desktop raw.vmdk grew to 80GB after a month of image churn. Prune images weekly, cap VM disk at 60GB to force cleanup.",
    area="infra", importance=3, tags=["docker", "gotcha"])

add(kind="learning", title="launchd vs cron on macOS",
    body="cron on macOS gets killed by system maintenance periodically and won't re-start. Use launchd plist with StartInterval or StartCalendarInterval instead.",
    area="infra", importance=4, tags=["macos", "launchd"])

add(kind="skill", title="SSH into server-mini",
    body="ssh server-mini via tailscale (user: admin). Key is ~/.ssh/id_ed25519_mesh. Password auth disabled. sudo requires password.",
    area="infra", importance=4, tags=["ssh"])

add(kind="skill", title="Home Assistant restart",
    body="On server-mini: cd /opt/homeassistant && docker compose restart. Takes ~20s. Web UI at http://server-mini:8123.",
    area="infra", importance=3, tags=["homeassistant"])

add(kind="skill", title="Tailscale exit node toggle",
    body="tailscale set --exit-node=server-mini to route through home. tailscale set --exit-node= to disable. Works from any machine on the tailnet.",
    area="infra", importance=3, tags=["tailscale"])

add(kind="rule", title="Never expose services without tailscale",
    body="Home services must only bind to the tailscale interface, not 0.0.0.0. Audit with ss -tulpn monthly.",
    area="infra", importance=5, tags=["security", "rule"])

add(kind="preference", title="Infra config in git",
    body="All home-lab configs (docker-compose, tailscale rules, launchd plists) live in a private git repo. Never edit in place on the host.",
    area="infra", importance=3, tags=["ops"])

add(kind="note", title="Kai upgraded Home Assistant",
    body="Kai upgraded Home Assistant from 2024.2 to 2024.4. One broken integration (z2m) needed a manual reconfigure. All zone automations still work.",
    area="infra", importance=2, tags=["homeassistant"])

add(kind="fact", title="Backup strategy",
    body="server-mini backs up config and HA data nightly via rclone to encrypted S3 bucket. Retention 30 days. Restore tested quarterly.",
    area="infra", importance=4, tags=["backup"])

add(kind="learning", title="Docker compose env file precedence",
    body="docker-compose picks up .env from the compose file's directory, not the current working directory. Was debugging for an hour why env vars weren't loading.",
    area="infra", importance=3, tags=["docker"])

# SUPERSEDED — old exit node was desktop-gpu
add(kind="fact", title="Old tailscale exit node",
    body="Exit node used to be desktop-gpu during 2023. Moved to server-mini because desktop-gpu was off overnight and broke the home-VPN use case.",
    area="infra", importance=1, tags=["tailscale", "history", "outdated"])


# ─────────────────────────────────────────────────────────────
# AUTOMATION (personal bots / scheduled tasks) — ~20 items
# ─────────────────────────────────────────────────────────────

add(kind="skill", title="Send Telegram message via bot",
    body="curl -X POST https://api.telegram.org/bot$TOKEN/sendMessage -d chat_id=$CHAT -d text='...'. Token in keychain as telegram-bot-token.",
    area="automation", importance=4, tags=["telegram"])

add(kind="skill", title="Home Assistant notify service",
    body="From HA: service notify.mobile_app_phone, data { message: '...', title: '...' }. From outside: POST /api/services/notify/mobile_app_phone with bearer token.",
    area="automation", importance=4, tags=["homeassistant", "notify"])

add(kind="fact", title="Telegram webhook endpoint",
    body="Telegram bot webhook points to server-mini:8443 via tailscale funnel. Cloudflared tunnel terminates TLS. Certificate auto-renewed.",
    area="automation", importance=3, tags=["telegram"])

add(kind="learning", title="HA automation debounce",
    body="Motion-triggered light automation fires multiple times if motion is intermittent. Added a debounce condition: state hasn't changed in the last 30s.",
    area="automation", importance=3, tags=["homeassistant"])

add(kind="skill", title="Add a new scheduled task",
    body="Edit ~/infra/launchd/com.user.<task>.plist, load with launchctl load -w <plist>. Logs in ~/Library/Logs/<task>.log.",
    area="automation", importance=3, tags=["launchd"])

add(kind="note", title="Morning routine automation",
    body="At 7:30am on weekdays: turn on bedroom lights gradually, play a wake-up playlist via Home Assistant, set coffee maker via smart plug.",
    area="automation", importance=2, tags=["routine"])

add(kind="fact", title="MQTT broker config",
    body="Mosquitto runs on server-mini, port 1883 internal, 8883 TLS for external. Clients auth with username/password from HA secrets file.",
    area="automation", importance=3, tags=["mqtt"])

add(kind="learning", title="Webhook retry strategy",
    body="Telegram bot webhook should respond 200 within 30s or Telegram retries. For long-running commands, ack immediately, process async, send message as reply.",
    area="automation", importance=4, tags=["telegram"])

add(kind="rule", title="No secrets in HA yaml",
    body="HA configuration yaml must reference secrets.yaml for tokens. Never paste API keys directly in config files. secrets.yaml is gitignored.",
    area="automation", importance=5, tags=["security", "rule"])

add(kind="preference", title="Notification tone",
    body="For automation notifications: prefer factual over cute. 'Motion detected in garage, 3:14am' not 'Hey! Something's happening!'.",
    area="automation", importance=2, tags=["style"])

add(kind="skill", title="Telegram command handler",
    body="Parse /commands in bot update handler. Command args follow after space. Prefix unknown commands with error message, don't crash.",
    area="automation", importance=3, tags=["telegram"])


# ─────────────────────────────────────────────────────────────
# ECHO (CLI terminal tool being built) — ~20 items
# ─────────────────────────────────────────────────────────────

add(kind="fact", title="Echo project stack",
    body="Echo is a Rust CLI with a Tauri v2 GUI wrapper. Backend Rust, frontend React + TypeScript, xterm.js for terminal rendering.",
    area="echo", importance=4, tags=["architecture"])

add(kind="fact", title="Echo PTY implementation",
    body="Echo uses portable-pty crate for cross-platform PTY. ConPTY on Windows, native on Unix. Each tab has its own PTY handle.",
    area="echo", importance=4, tags=["pty"])

add(kind="learning", title="parking_lot Mutex not reentrant",
    body="parking_lot::Mutex is not reentrant. Calling .lock() while already holding the lock on the same thread deadlocks immediately.",
    area="echo", importance=5, tags=["rust", "gotcha"])

add(kind="learning", title="xterm.js renderer choice",
    body="xterm.js default canvas renderer is slow for 80+ columns. Switched to WebGL renderer (addon), 3x faster repaints but breaks on some old GPUs.",
    area="echo", importance=3, tags=["xterm"])

add(kind="skill", title="Echo build command",
    body="./build.sh dev starts dev server with hot reload. ./build.sh build produces production artifacts. ./build.sh test runs unit tests.",
    area="echo", importance=4, tags=["dev"])

add(kind="rule", title="Never kill production echo",
    body="When killing a dev process, identify by port (dev=7686, prod=7685). Only kill dev PID. Accidentally killing prod loses user session data.",
    area="echo", importance=5, tags=["safety", "rule"])

add(kind="fact", title="Echo session storage",
    body="Echo persists sessions (tabs, cwd, command history) to ~/.echo/sessions/last.json. Reloaded on next start unless --new-session flag.",
    area="echo", importance=3, tags=["persistence"])

add(kind="learning", title="Tauri v2 plugin architecture change",
    body="Tauri v2 moved from invoke/invoke_handler to the new plugin system. Old v1 commands need refactoring as plugin commands or exposed via the core namespace.",
    area="echo", importance=3, tags=["tauri"])

add(kind="preference", title="Echo commit scope",
    body="Commit messages use scopes: feat(core), fix(ui), refactor(pty), etc. Breaking changes prefixed with !. Follows conventional commits.",
    area="echo", importance=2, tags=["git"])

add(kind="skill", title="Echo release process",
    body="Bump version in Cargo.toml and package.json, tag v<version>, push tag. CI builds artifacts for macOS, Windows, Linux and uploads to GH release.",
    area="echo", importance=3, tags=["release"])

add(kind="note", title="Switched from serde_json to simd-json",
    body="Profiling showed JSON parsing was 8% of startup time for large session files. Switched to simd-json, now 2%. No behavior changes.",
    area="echo", importance=2, tags=["perf"])

add(kind="learning", title="React 18 StrictMode double effect",
    body="React 18 StrictMode runs effects twice in dev. Caused duplicate WebSocket connections. Guard with a useRef<boolean> or move to initializer.",
    area="echo", importance=4, tags=["react"])

add(kind="rule", title="No unwrap in library code",
    body="Library crates must not contain .unwrap() or .expect() unless it's a documented invariant. Return Result<T, Error> and propagate.",
    area="echo", importance=4, tags=["rust", "rule"])

add(kind="fact", title="Echo keybinding config",
    body="Keybindings configurable via ~/.echo/config.toml [keybinds] section. Defaults: Ctrl+T new tab, Ctrl+W close tab, Ctrl+Shift+P command palette.",
    area="echo", importance=3, tags=["config"])

add(kind="conversation", title="Discussion about plugin API",
    body="Debated whether to expose a Wasm plugin runtime or just native dylib plugins. Decided: start with native, revisit Wasm after core is stable.",
    area="echo", importance=2, tags=["planning"])


# ─────────────────────────────────────────────────────────────
# PERSONAL preferences + identity — ~15 items
# ─────────────────────────────────────────────────────────────

add(kind="preference", title="Editor",
    body="Primary editor is VSCode with vim keybindings. Secondary is vim for quick edits. Font: JetBrains Mono 14pt. Theme: Dracula.",
    area="personal", importance=3, tags=["editor"])

add(kind="preference", title="Terminal theme",
    body="Terminal uses Dracula color scheme. Background slightly transparent (90%). Font: JetBrains Mono 13pt.",
    area="personal", importance=2, tags=["terminal"])

add(kind="preference", title="Response style",
    body="Prefer concise responses. No padding, no 'great question', no 'happy to help'. Assume technical baseline. Sharp direct feedback.",
    area="personal", importance=5, tags=["communication"])

add(kind="preference", title="Language",
    body="Code, comments, and docs in English only. Chat replies follow user's language. Thinking/reasoning always English for token efficiency.",
    area="personal", importance=5, tags=["communication"])

add(kind="rule", title="Never commit without explicit request",
    body="Do not create git commits unless user explicitly asks. No auto-commits at the end of tasks. Let the user review and decide.",
    area="personal", importance=5, tags=["git", "rule"])

add(kind="rule", title="Don't add unnecessary comments",
    body="Default to no comments. Only add when the WHY is non-obvious. Never comment what the code does — naming should convey that.",
    area="personal", importance=4, tags=["style", "rule"])

add(kind="preference", title="Task style",
    body="When given a task, dive in and do it. Ask only if truly ambiguous or destructive. Report findings at the end, not a running commentary.",
    area="personal", importance=4, tags=["workflow"])

add(kind="preference", title="Test before reporting done",
    body="Run relevant unit tests, type checks, or builds before declaring a task complete. Blocked/partial is acceptable, silent broken is not.",
    area="personal", importance=4, tags=["workflow"])

add(kind="rule", title="No emoji in code or docs",
    body="Don't add emojis to code, comments, commit messages, or documentation files unless explicitly requested. Emojis acceptable in chat replies only when the user uses them.",
    area="personal", importance=3, tags=["style", "rule"])

add(kind="preference", title="Commit message format",
    body="Conventional commits style: <type>(<scope>): <subject>. Body explains why, not what. No emojis. No Co-Authored-By unless pair programming.",
    area="personal", importance=3, tags=["git"])


# ─────────────────────────────────────────────────────────────
# Daily / summary items (simulate past dream diaries) — ~10 items
# ─────────────────────────────────────────────────────────────

add(kind="daily", title="Dream diary 2026-04-04",
    body="Today: worked on Beacon Postgres slow query fix. Talked to Jordan about Django 5 upgrade. Consolidated 3 duplicate PgBouncer facts into canonical config item.",
    area=None, importance=2, tags=["dream-diary"])

add(kind="daily", title="Dream diary 2026-04-05",
    body="Mostly Aurora work. Alex proposed mixer refactor. Morgan shared onboarding mockups. Fixed StoreKit transaction finishing bug. 4 new learnings added.",
    area=None, importance=2, tags=["dream-diary"])

add(kind="daily", title="Dream diary 2026-04-06",
    body="Cobalt dashboard work with Riley. Playwright flake investigation. Discussed RSC and decided to postpone. 2 duplicate React Query notes merged.",
    area=None, importance=2, tags=["dream-diary"])

add(kind="daily", title="Dream diary 2026-04-07",
    body="Infra day. Kai upgraded Home Assistant. Fixed docker disk usage. Added launchd task for nightly backups. No new duplicates.",
    area=None, importance=2, tags=["dream-diary"])

add(kind="summary", title="Q1 themes",
    body="Q1 dominant themes: Beacon transcription reliability (Deepgram→Whisper fallback), Aurora Observation framework migration, Cobalt bundle size reduction. 3 major postmortems, 2 regressions fixed.",
    area=None, importance=3, tags=["summary", "quarter"])

add(kind="summary", title="Persistent concerns",
    body="Recurring pain points across projects: Celery task idempotency (Beacon), Audio callback thread safety (Aurora), Okta session expiry UX (Cobalt). Each has open tickets and work ongoing.",
    area=None, importance=3, tags=["summary"])


# ─────────────────────────────────────────────────────────────
# A few scripts (short code snippets as skills)
# ─────────────────────────────────────────────────────────────

add(kind="script", title="Quick Postgres slow query finder",
    body="SELECT query, calls, mean_exec_time FROM pg_stat_statements ORDER BY mean_exec_time DESC LIMIT 20; — requires pg_stat_statements extension enabled.",
    area="beacon", importance=3, tags=["postgres", "sql"])

add(kind="script", title="Docker cleanup one-liner",
    body="docker system prune -af --volumes — removes all unused images, containers, networks, volumes. Use with caution, no confirmation.",
    area="infra", importance=3, tags=["docker"])

add(kind="script", title="Kill process on port",
    body="lsof -ti :<port> | xargs kill -9 — kills whatever is listening on that port. On macOS and Linux. For Windows use netstat -ano findstr :<port>.",
    area="infra", importance=3, tags=["unix"])


# ─────────────────────────────────────────────────────────────
# EXTRA aurora — edge cases, more dupes
# ─────────────────────────────────────────────────────────────

add(kind="learning", title="Sandbox StoreKit testing",
    body="StoreKit.configuration file in Xcode enables local IAP testing without sandbox account. Useful for unit tests. Doesn't simulate network errors though.",
    area="aurora", importance=3, tags=["storekit", "testing"])

add(kind="learning", title="SwiftUI NavigationStack deep link",
    body="NavigationStack with bound path array can deep-link anywhere in the hierarchy. Setting path = [.profile, .settings] programmatically navigates two levels deep.",
    area="aurora", importance=3, tags=["swiftui"])

add(kind="note", title="Alex found bug in mixer",
    body="Alex found that enabling more than 3 audio layers caused a crash on iPhone SE 2nd gen due to CPU pressure. Added a device-based limit.",
    area="aurora", importance=3, tags=["bug"])

add(kind="fact", title="Aurora audio presets",
    body="Aurora ships with 24 built-in audio presets: rain, ocean, forest, fire, white noise variants, and guided breathing. New presets monthly.",
    area="aurora", importance=3, tags=["content"])

add(kind="fact", title="Aurora analytics provider",
    body="Aurora uses Mixpanel for product analytics and Sentry for crash reporting. No PII sent — user id is a hashed device identifier.",
    area="aurora", importance=3, tags=["analytics"])

add(kind="skill", title="Debug Aurora crash from Sentry",
    body="Open Sentry issue, grab the symbolicated stack trace, check device + iOS version. Reproduce in simulator with same iOS. Most crashes are in the legacy player VC.",
    area="aurora", importance=3, tags=["debugging"])

add(kind="rule", title="All Aurora PRs need a test",
    body="PRs touching business logic or audio pipeline require at least one new test. UI-only or config changes are exempt. CI blocks PRs missing tests.",
    area="aurora", importance=4, tags=["testing", "rule"])

add(kind="learning", title="URLSession background downloads",
    body="Background URLSession downloads survive app termination but not device reboot. Register for restoration via UIApplicationDelegate handleEventsForBackgroundURLSession.",
    area="aurora", importance=3, tags=["networking"])

add(kind="preference", title="Aurora code review style",
    body="Code review: be specific, cite exact lines, suggest alternatives not just criticism. Approve as soon as concerns addressed, don't nitpick formatting.",
    area="aurora", importance=2, tags=["review"])

add(kind="fact", title="Aurora crash grouping",
    body="Sentry crash grouping set to 'frame in app' + exception type. Prevents one user's weird crash from dominating alerts with identical noise.",
    area="aurora", importance=2, tags=["sentry"])


# ─────────────────────────────────────────────────────────────
# EXTRA beacon — infra + incidents
# ─────────────────────────────────────────────────────────────

add(kind="note", title="Postgres connection spike incident",
    body="Saturday: Postgres connection count hit limit of 400 for 15 minutes. Root cause: runaway Celery worker holding connections open. Fixed by adding connection timeout.",
    area="beacon", importance=4, tags=["incident"])

add(kind="learning", title="Django ORM select_related over-fetch",
    body="select_related('*') generates massive SQL joins. Be explicit about which relations. Saved 200ms per meeting list request by dropping unused prefetches.",
    area="beacon", importance=4, tags=["django", "perf"])

add(kind="learning", title="Celery prefetch multiplier",
    body="Default Celery worker_prefetch_multiplier=4 grabs too many tasks and creates head-of-line blocking. Set to 1 for long-running tasks. Trade throughput for fairness.",
    area="beacon", importance=4, tags=["celery"])

add(kind="fact", title="Beacon Redis usage",
    body="Redis used for Celery broker, result backend, Django cache, and transcript intermediate cache. 8GB instance, maxmemory-policy allkeys-lru.",
    area="beacon", importance=3, tags=["redis"])

add(kind="fact", title="Beacon S3 buckets",
    body="Three S3 buckets: raw-audio (meeting recordings), transcripts (JSON + SRT), exports (user downloads). Lifecycle rule archives raw-audio to Glacier after 90 days.",
    area="beacon", importance=3, tags=["s3", "storage"])

add(kind="skill", title="Rerun failed Celery task",
    body="python manage.py shell, then from celery.result import AsyncResult; AsyncResult(task_id).backend.restore_task(task_id). Or resubmit via the task signature.",
    area="beacon", importance=3, tags=["celery"])

add(kind="skill", title="Backup Beacon DB locally",
    body="pg_dump -h beacon-staging -U beacon -Fc beacon > backup.dump. Restore with pg_restore -d target backup.dump. Takes ~3 minutes for staging.",
    area="beacon", importance=3, tags=["db"])

add(kind="rule", title="No blocking IO in async Django views",
    body="Async Django views must not call sync ORM or requests. Wrap in sync_to_async or use async ORM (Django 4.2+). CI lint rule catches most cases.",
    area="beacon", importance=4, tags=["django", "async", "rule"])

add(kind="note", title="Taylor benchmark report",
    body="Taylor ran a benchmark comparing Whisper, Deepgram, and AssemblyAI on 100 sample meetings. Deepgram: 92% WER-adjusted accuracy, cheapest. Whisper: 89%, zero cost.",
    area="beacon", importance=3, tags=["ml", "benchmark"])

add(kind="conversation", title="Discussion about on-prem deployment",
    body="Enterprise customer asked about on-prem deployment. Team decided to offer a docker-compose bundle for proof-of-concept, not full support yet.",
    area="beacon", importance=2, tags=["enterprise"])

add(kind="fact", title="Beacon SLA",
    body="99.9% uptime SLA for Pro and Enterprise. Free tier best-effort, no SLA. Incidents tracked on status.beacon.internal.",
    area="beacon", importance=3, tags=["sla"])

add(kind="learning", title="Django atomic transaction gotcha",
    body="@transaction.atomic around a block that sends a Celery task: the task can pick up the row before the transaction commits. Use transaction.on_commit(lambda: task.delay()) instead.",
    area="beacon", importance=5, tags=["django", "celery", "gotcha"])

add(kind="skill", title="Tail Celery task failures",
    body="python manage.py shell: from django_celery_results.models import TaskResult; TaskResult.objects.filter(status='FAILURE').order_by('-date_done')[:20]",
    area="beacon", importance=3, tags=["celery", "debugging"])


# ─────────────────────────────────────────────────────────────
# EXTRA cobalt — perf, tooling
# ─────────────────────────────────────────────────────────────

add(kind="skill", title="Cobalt bundle analyze",
    body="pnpm build:analyze produces a treemap of bundle contents. Serves at http://localhost:8888. Useful for finding unexpectedly large dependencies.",
    area="cobalt", importance=3, tags=["perf", "tooling"])

add(kind="learning", title="Tailwind JIT purge",
    body="Tailwind JIT purges unused classes only if file paths in tailwind.config.js cover them. Missed src/plugins/* and half the plugin UI was unstyled in prod.",
    area="cobalt", importance=3, tags=["tailwind", "gotcha"])

add(kind="note", title="Bundle size investigation",
    body="Found that lodash was imported in 12 places via barrel file, pulling the whole lib. Switched to per-function imports (lodash/pick, lodash/debounce). Saved 60KB.",
    area="cobalt", importance=3, tags=["perf"])

add(kind="fact", title="Cobalt error boundary",
    body="Top-level error boundary catches React rendering errors, shows a friendly UI with a retry button, logs to Sentry with component stack.",
    area="cobalt", importance=3, tags=["react"])

add(kind="learning", title="Zustand shallow compare",
    body="useStore((s) => ({a: s.a, b: s.b})) creates a new object every render and causes infinite loops. Use shallow comparator: useStore(selector, shallow).",
    area="cobalt", importance=4, tags=["zustand", "gotcha"])

add(kind="rule", title="Keep Cobalt PRs under 500 lines",
    body="Large PRs get bounced back with a request to split. Review quality drops above 500 lines of diff. Exception: pure refactors with automated codemods.",
    area="cobalt", importance=3, tags=["review", "rule"])


# ─────────────────────────────────────────────────────────────
# EXTRA infra + automation
# ─────────────────────────────────────────────────────────────

add(kind="learning", title="Tailscale funnel certificate",
    body="Tailscale Funnel provisions a Let's Encrypt cert automatically but only for *.tailnet-name.ts.net domains. Custom domains need a separate reverse proxy.",
    area="infra", importance=3, tags=["tailscale"])

add(kind="learning", title="systemd-like launchd daemons",
    body="launchd KeepAlive true auto-restarts the daemon on crash, but only after a throttle interval (10s). SuccessfulExit false is often what you actually want.",
    area="infra", importance=3, tags=["launchd"])

add(kind="skill", title="Inspect docker network",
    body="docker network inspect <name> shows connected containers, subnet, gateway. docker network prune removes all unused networks.",
    area="infra", importance=2, tags=["docker"])

add(kind="note", title="Kai proposed mesh monitoring",
    body="Kai wants to set up Prometheus on server-mini to scrape metrics from all tailnet machines. Needs node_exporter on each. Planned for next month.",
    area="infra", importance=2, tags=["monitoring"])

add(kind="fact", title="DNS setup",
    body="Internal DNS via tailscale MagicDNS. No manual Pi-hole or coredns. Lookup format: <machine>.<tailnet>.ts.net. Works across OS.",
    area="infra", importance=3, tags=["dns"])

add(kind="learning", title="docker-compose healthcheck gotcha",
    body="depends_on: service_healthy only works if the depended-on service has a healthcheck defined. Without it, depends_on is just a start-order hint.",
    area="infra", importance=3, tags=["docker"])

add(kind="skill", title="Diagnose high CPU on server-mini",
    body="ssh server-mini && top -o cpu. Or htop if installed. For per-container: docker stats --no-stream. Usually the culprit is a runaway HA automation.",
    area="infra", importance=3, tags=["ops"])

add(kind="note", title="Backup restore drill",
    body="Quarterly drill: restore HA backup to a VM, verify automations and integrations load. Last drill: all clean except one broken template sensor (manual fix).",
    area="infra", importance=3, tags=["backup", "drill"])

add(kind="fact", title="Mesh bandwidth budget",
    body="Tailnet peak bandwidth is ~50Mbps (upload-bound by home ISP). Heavy sync tasks (ML datasets) scheduled for 3am-6am off-peak.",
    area="infra", importance=2, tags=["mesh"])


# ─────────────────────────────────────────────────────────────
# EXTRA echo (CLI tool)
# ─────────────────────────────────────────────────────────────

add(kind="fact", title="Echo agent providers",
    body="Echo supports multiple agent providers: Claude, Codex, Gemini. Each implemented as a trait in src/plugin/. Detection via OSC title and session files.",
    area="echo", importance=4, tags=["architecture"])

add(kind="learning", title="OSC 7 cwd tracking",
    body="OSC 7 escape sequence reports current working directory from shell to terminal. Bash and zsh emit it natively. For PowerShell, inject via prompt function.",
    area="echo", importance=3, tags=["pty"])

add(kind="note", title="Rust parallel compile investigation",
    body="Echo Rust build takes 4 minutes cold. Enabled sparse registry, cargo chef for CI layer caching, and jobs=8. Cold build now 90s.",
    area="echo", importance=3, tags=["rust", "perf"])

add(kind="learning", title="Tokio runtime choice",
    body="Tokio multi-thread runtime shines for IO-bound workloads. For echo's single-user event loop, current-thread runtime is actually faster (no work stealing overhead).",
    area="echo", importance=3, tags=["tokio"])

add(kind="skill", title="Echo unit test pattern",
    body="Tests inline in #[cfg(test)] mod tests { } at bottom of source file. Use tempfile::tempdir() for filesystem tests. Avoid real network — mock or bypass.",
    area="echo", importance=3, tags=["testing", "rust"])

add(kind="rule", title="Lock ordering in echo",
    body="Acquire locks in order: mux → output_buf → screen_state → agent_tracker → pty. Never hold higher-numbered while acquiring lower. Deadlock prevention.",
    area="echo", importance=5, tags=["concurrency", "rule"])


# ─────────────────────────────────────────────────────────────
# Extra notes to fill out
# ─────────────────────────────────────────────────────────────

add(kind="note", title="Weekend: tried Nix on mac",
    body="Spent Saturday trying to set up Nix for reproducible dev environments. Got a basic flake working. Too much overhead for personal use, parked the experiment.",
    area="personal", importance=1, tags=["nix", "experiment"])

add(kind="note", title="Read paper on speculative decoding",
    body="Read recent paper on speculative decoding for LLM inference. Technique can ~2x decode speed by running a small draft model alongside the big one. Interesting for Beacon.",
    area="personal", importance=2, tags=["ml", "reading"])

add(kind="learning", title="macOS SIP and system modifications",
    body="macOS System Integrity Protection (SIP) blocks writes to /System and /usr. Legitimate system mods require boot to Recovery and csrutil disable. Don't do it.",
    area="infra", importance=3, tags=["macos"])

add(kind="skill", title="Generate SSH key pair",
    body="ssh-keygen -t ed25519 -C 'comment' -f ~/.ssh/keyname. Add to ssh-agent: ssh-add ~/.ssh/keyname. Copy public to remote ~/.ssh/authorized_keys.",
    area="infra", importance=3, tags=["ssh"])

add(kind="fact", title="Dotfiles repo",
    body="All dotfiles in ~/dotfiles, symlinked via stow. Zsh config, git config, VSCode settings, tmux conf. Private keys not included.",
    area="personal", importance=2, tags=["dotfiles"])

add(kind="skill", title="Profile Python memory",
    body="memory_profiler + @profile decorator shows line-by-line memory usage. tracemalloc for snapshot comparison. Useful for finding leaks in long-running workers.",
    area="beacon", importance=2, tags=["python", "perf"])

add(kind="rule", title="Always pin Docker base images",
    body="Never use :latest in production Dockerfiles. Pin to specific tags or digests. Reproducibility > convenience.",
    area="infra", importance=4, tags=["docker", "rule"])

add(kind="preference", title="Reading list format",
    body="Keep a list of papers/articles to read in a plain text file. One line per item, prefix with [x] when done. No app, no database, just text.",
    area="personal", importance=1, tags=["reading"])

add(kind="note", title="Experimented with local models",
    body="Ran Llama 3.1 70B on desktop-gpu via ollama. Usable for chat, struggles with long context. Keeps memory for follow-ups better than expected.",
    area="personal", importance=2, tags=["llm", "experiment"])


# ─────────────────────────────────────────────────────────────
# Counts
# ─────────────────────────────────────────────────────────────
if __name__ == "__main__":
    from collections import Counter
    kinds = Counter(item["kind"] for item in ITEMS)
    areas = Counter(item.get("area") or "(none)" for item in ITEMS)
    print(f"Total items: {len(ITEMS)}")
    print("\nBy kind:")
    for k, v in sorted(kinds.items(), key=lambda x: -x[1]):
        print(f"  {k:14} {v}")
    print("\nBy area:")
    for a, v in sorted(areas.items(), key=lambda x: -x[1]):
        print(f"  {a:14} {v}")
