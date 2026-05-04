"""Auto-split persona seeds (was inline in loaded.py).

Use _add() to append to this module's ITEMS — each persona keeps its own list,
which the loaded package concatenates in __init__.py.
"""

ITEMS: list[dict] = []


def _add(**kw):
    ITEMS.append(kw)


# ─────────────────────────────────────────────────────────────
# BEACON (AI meeting assistant, Python backend) — ~35 items
# ─────────────────────────────────────────────────────────────

_add(kind="fact", title="Beacon PgBouncer config",
    body="Beacon uses PgBouncer in transaction pooling mode with max_client_conn=400. Sits in front of the primary Postgres. One bouncer per region.",
    area="beacon", importance=5, tags=["pgbouncer", "db"])

# NEAR-DUPLICATE (wording 2)
_add(kind="fact", title="Beacon connection pooler",
    body="Connection pooling for Beacon's Postgres is done via PgBouncer running in transaction mode. Max client connections set to 400.",
    area="beacon", importance=3, tags=["pgbouncer"])

# NEAR-DUPLICATE (wording 3)
_add(kind="fact", title="Beacon database pooling",
    body="Postgres connections for Beacon are routed through PgBouncer with transaction-level pooling. Configured for 400 concurrent clients.",
    area="beacon", importance=3, tags=["db", "pooling"])

_add(kind="fact", title="Beacon stack overview",
    body="Beacon backend is Django 5 + Celery + Redis + Postgres. LangGraph for agent orchestration. WebRTC via LiveKit for real-time audio capture.",
    area="beacon", importance=5, tags=["architecture"])

_add(kind="fact", title="Beacon transcription provider",
    body="Beacon uses Deepgram Nova-2 for primary transcription. Falls back to Whisper large-v3 via internal GPU worker if Deepgram rate-limits.",
    area="beacon", importance=4, tags=["transcription"])

_add(kind="fact", title="Beacon auth provider",
    body="Beacon uses Auth0 for SSO. JWT tokens validated via JWKS endpoint. Refresh tokens stored httpOnly cookies.",
    area="beacon", importance=4, tags=["auth"])

_add(kind="learning", title="Auth0 JWT kid header requirement",
    body="Auth0 requires the kid header in JWT even though RFC says it's optional. Without kid, JWKS lookup fails silently and every request comes back 401.",
    area="beacon", importance=5, tags=["auth", "jwt", "gotcha"])

# NEAR-DUPLICATE of the auth0 learning above
_add(kind="learning", title="JWT refresh 401 fix",
    body="Auth0 token refresh was returning 401 until we added the kid header to JWT construction. Spec marks kid optional but Auth0 needs it.",
    area="beacon", importance=3, tags=["jwt", "fix"])

_add(kind="learning", title="LangGraph state persistence",
    body="LangGraph checkpointer must use a persistent backend (Postgres, not in-memory) for multi-step agent flows. In-memory checkpointer loses state between worker restarts.",
    area="beacon", importance=4, tags=["langgraph"])

_add(kind="learning", title="LiveKit room cleanup",
    body="LiveKit rooms must be explicitly deleted via API after the meeting ends. Auto-cleanup on empty-for-10min doesn't trigger if any participant reconnects briefly.",
    area="beacon", importance=3, tags=["livekit"])

_add(kind="note", title="Jordan proposed moving to Django 5",
    body="Jordan wants to upgrade Django 4 → 5 for async views support. Estimated 2-3 days of work. Concern: some Celery tasks use sync db adapter.",
    area="beacon", importance=2, tags=["django"])

_add(kind="note", title="Taylor improved Whisper latency",
    body="Taylor reduced Whisper inference latency by ~30% by switching to faster-whisper and enabling CUDA graphs. Baseline 2.1s → 1.45s for 30s audio.",
    area="beacon", importance=3, tags=["ml", "perf"])

_add(kind="skill", title="Beacon local dev",
    body="docker compose up in beacon/ starts postgres, redis, pgbouncer, django, celery worker. Then python manage.py runserver. Django admin at /admin.",
    area="beacon", importance=4, tags=["dev"])

_add(kind="skill", title="Beacon deploy to staging",
    body="Push to release/staging branch. GitHub Actions builds image, pushes to ECR, triggers ECS task deploy. Takes ~6 minutes end-to-end.",
    area="beacon", importance=3, tags=["deploy"])

_add(kind="skill", title="How to tail Beacon Celery logs",
    body="ssh beacon-staging, then docker logs -f beacon_celery_1. Or via aws logs tail /ecs/beacon-celery --follow.",
    area="beacon", importance=2, tags=["ops"])

_add(kind="rule", title="Never commit AWS keys",
    body="Any commit containing AWS_SECRET_ACCESS_KEY triggers a pre-commit hook block. If accidentally committed, rotate the key immediately — even if the repo is private.",
    area="beacon", importance=5, tags=["security"])

_add(kind="rule", title="Celery tasks must be idempotent",
    body="All Celery tasks must be safe to retry. Use database constraints and external idempotency keys for side-effect calls. Acks_late + task_reject_on_worker_lost enabled.",
    area="beacon", importance=4, tags=["celery", "rule"])

_add(kind="preference", title="Beacon Python style",
    body="Black + ruff + mypy strict. Line length 100. Type hints on every function. No implicit Optional. Imports grouped by isort.",
    area="beacon", importance=2, tags=["style"])

_add(kind="fact", title="Beacon LangGraph agent count",
    body="Beacon's LangGraph pipeline has 7 nodes: ingest, diarize, transcribe, align, summarize, extract_actions, format_output. Runs in parallel where DAG allows.",
    area="beacon", importance=4, tags=["langgraph"])

_add(kind="fact", title="Beacon Postgres size",
    body="Primary Postgres is db.r6g.xlarge with 500GB gp3. ~40GB used for user data, ~120GB for transcripts, rest is indexes and WAL.",
    area="beacon", importance=3, tags=["db"])

_add(kind="learning", title="Deepgram websocket reconnect",
    body="Deepgram streaming WS disconnects silently after ~3min of silence. Must send a keepalive frame every 60s or reconnect on the next audio chunk.",
    area="beacon", importance=4, tags=["deepgram", "gotcha"])

_add(kind="learning", title="Redis OOM from transcript cache",
    body="Transcript intermediate cache with no TTL filled Redis in ~8 hours of usage. Set TTL 2h on the cache keys, added maxmemory-policy allkeys-lru as safety net.",
    area="beacon", importance=4, tags=["redis"])

# SUPERSEDED pair
_add(kind="fact", title="Beacon old transcription provider",
    body="Before Deepgram, Beacon used AssemblyAI for transcription. Switched last quarter due to cost and worse accuracy on accented speech.",
    area="beacon", importance=1, tags=["transcription", "history", "outdated"])

_add(kind="note", title="Jordan reported Postgres slow query",
    body="Jordan flagged a slow query on the meetings list endpoint — takes 8s with >1000 meetings. Needs index on (user_id, created_at DESC).",
    area="beacon", importance=3, tags=["db", "perf"])

_add(kind="skill", title="Beacon DB migration",
    body="python manage.py makemigrations, then migrate. On prod: always run with --plan first to preview. Never edit historical migration files.",
    area="beacon", importance=3, tags=["db", "migration"])

_add(kind="conversation", title="Team discussion about meeting length limit",
    body="Current limit is 4 hours. Team wants to raise to 8h. Concern: transcript memory grows O(n) and summarization model context window is the hard cap.",
    area="beacon", importance=2, tags=["planning"])

_add(kind="fact", title="Beacon rate limit per user",
    body="Free tier: 10 meetings per month. Pro: unlimited meetings, 4h each. Enterprise: unlimited with custom length limit.",
    area="beacon", importance=3, tags=["pricing", "limits"])

_add(kind="learning", title="Celery beat scheduler on redeploy",
    body="Celery beat loses its schedule on container restart unless using django-celery-beat with DB persistence. Otherwise scheduled tasks get double-run during rolling deploys.",
    area="beacon", importance=4, tags=["celery"])

_add(kind="fact", title="Beacon regional deployment",
    body="Beacon runs in us-east-1 primary, eu-west-1 replica. DB replication lag typically <200ms. Failover is manual via Route53 weight change.",
    area="beacon", importance=3, tags=["infra"])

# ─────────────────────────────────────────────────────────────
# EXTRA beacon — infra + incidents
# ─────────────────────────────────────────────────────────────

_add(kind="note", title="Postgres connection spike incident",
    body="Saturday: Postgres connection count hit limit of 400 for 15 minutes. Root cause: runaway Celery worker holding connections open. Fixed by adding connection timeout.",
    area="beacon", importance=4, tags=["incident"])

_add(kind="learning", title="Django ORM select_related over-fetch",
    body="select_related('*') generates massive SQL joins. Be explicit about which relations. Saved 200ms per meeting list request by dropping unused prefetches.",
    area="beacon", importance=4, tags=["django", "perf"])

_add(kind="learning", title="Celery prefetch multiplier",
    body="Default Celery worker_prefetch_multiplier=4 grabs too many tasks and creates head-of-line blocking. Set to 1 for long-running tasks. Trade throughput for fairness.",
    area="beacon", importance=4, tags=["celery"])

_add(kind="fact", title="Beacon Redis usage",
    body="Redis used for Celery broker, result backend, Django cache, and transcript intermediate cache. 8GB instance, maxmemory-policy allkeys-lru.",
    area="beacon", importance=3, tags=["redis"])

_add(kind="fact", title="Beacon S3 buckets",
    body="Three S3 buckets: raw-audio (meeting recordings), transcripts (JSON + SRT), exports (user downloads). Lifecycle rule archives raw-audio to Glacier after 90 days.",
    area="beacon", importance=3, tags=["s3", "storage"])

_add(kind="skill", title="Rerun failed Celery task",
    body="python manage.py shell, then from celery.result import AsyncResult; AsyncResult(task_id).backend.restore_task(task_id). Or resubmit via the task signature.",
    area="beacon", importance=3, tags=["celery"])

_add(kind="skill", title="Backup Beacon DB locally",
    body="pg_dump -h beacon-staging -U beacon -Fc beacon > backup.dump. Restore with pg_restore -d target backup.dump. Takes ~3 minutes for staging.",
    area="beacon", importance=3, tags=["db"])

_add(kind="rule", title="No blocking IO in async Django views",
    body="Async Django views must not call sync ORM or requests. Wrap in sync_to_async or use async ORM (Django 4.2+). CI lint rule catches most cases.",
    area="beacon", importance=4, tags=["django", "async", "rule"])

_add(kind="note", title="Taylor benchmark report",
    body="Taylor ran a benchmark comparing Whisper, Deepgram, and AssemblyAI on 100 sample meetings. Deepgram: 92% WER-adjusted accuracy, cheapest. Whisper: 89%, zero cost.",
    area="beacon", importance=3, tags=["ml", "benchmark"])

_add(kind="conversation", title="Discussion about on-prem deployment",
    body="Enterprise customer asked about on-prem deployment. Team decided to offer a docker-compose bundle for proof-of-concept, not full support yet.",
    area="beacon", importance=2, tags=["enterprise"])

_add(kind="fact", title="Beacon SLA",
    body="99.9% uptime SLA for Pro and Enterprise. Free tier best-effort, no SLA. Incidents tracked on status.beacon.internal.",
    area="beacon", importance=3, tags=["sla"])

_add(kind="learning", title="Django atomic transaction gotcha",
    body="@transaction.atomic around a block that sends a Celery task: the task can pick up the row before the transaction commits. Use transaction.on_commit(lambda: task.delay()) instead.",
    area="beacon", importance=5, tags=["django", "celery", "gotcha"])

_add(kind="skill", title="Tail Celery task failures",
    body="python manage.py shell: from django_celery_results.models import TaskResult; TaskResult.objects.filter(status='FAILURE').order_by('-date_done')[:20]",
    area="beacon", importance=3, tags=["celery", "debugging"])
