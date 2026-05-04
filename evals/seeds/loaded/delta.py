"""Auto-split persona seeds (was inline in loaded.py).

Use _add() to append to this module's ITEMS — each persona keeps its own list,
which the loaded package concatenates in __init__.py.
"""

ITEMS: list[dict] = []


def _add(**kw):
    ITEMS.append(kw)


# ─────────────────────────────────────────────────────────────
# DELTA (home-lab infra / mesh) — ~25 items
# ─────────────────────────────────────────────────────────────

_add(kind="peer", title="laptop-main",
    body="Primary dev laptop, macOS 15. Runs local dev servers, VSCode, Docker Desktop. On tailscale mesh as laptop-main.",
    area="infra", importance=4, tags=["machine"])

_add(kind="peer", title="desktop-gpu",
    body="Desktop workstation with a large GPU, Windows 11. Used for ML training and heavy CI. Reachable via tailscale as desktop-gpu.",
    area="infra", importance=4, tags=["machine"])

_add(kind="peer", title="server-mini",
    body="Mac mini M2, always on, hosts Home Assistant, Mosquitto, internal bots, Docker services. Reachable as server-mini.",
    area="infra", importance=5, tags=["machine"])

_add(kind="peer", title="mobile-dev",
    body="Secondary MacBook Pro, used for mobile builds and testing. On tailscale as mobile-dev. Xcode and Android Studio installed.",
    area="infra", importance=3, tags=["machine"])

_add(kind="fact", title="Mesh topology",
    body="All machines are on a tailscale tailnet. server-mini acts as exit node for the home subnet. MagicDNS enabled, no manual IPs needed.",
    area="infra", importance=4, tags=["tailscale", "mesh"])

_add(kind="fact", title="Home Assistant config location",
    body="Home Assistant lives on server-mini in docker compose. Config at /opt/homeassistant/config. Backed up nightly to S3 via rclone.",
    area="infra", importance=4, tags=["homeassistant"])

_add(kind="fact", title="Tailscale ACL policy",
    body="Tailnet ACL restricts dev laptops from accessing the IoT subnet (192.168.10.0/24). Only server-mini can bridge. Policy in tailnet admin console.",
    area="infra", importance=4, tags=["tailscale", "security"])

_add(kind="learning", title="Docker Desktop VM eating disk",
    body="Docker Desktop raw.vmdk grew to 80GB after a month of image churn. Prune images weekly, cap VM disk at 60GB to force cleanup.",
    area="infra", importance=3, tags=["docker", "gotcha"])

_add(kind="learning", title="launchd vs cron on macOS",
    body="cron on macOS gets killed by system maintenance periodically and won't re-start. Use launchd plist with StartInterval or StartCalendarInterval instead.",
    area="infra", importance=4, tags=["macos", "launchd"])

_add(kind="skill", title="SSH into server-mini",
    body="ssh server-mini via tailscale (user: admin). Key is ~/.ssh/id_ed25519_mesh. Password auth disabled. sudo requires password.",
    area="infra", importance=4, tags=["ssh"])

_add(kind="skill", title="Home Assistant restart",
    body="On server-mini: cd /opt/homeassistant && docker compose restart. Takes ~20s. Web UI at http://server-mini:8123.",
    area="infra", importance=3, tags=["homeassistant"])

_add(kind="skill", title="Tailscale exit node toggle",
    body="tailscale set --exit-node=server-mini to route through home. tailscale set --exit-node= to disable. Works from any machine on the tailnet.",
    area="infra", importance=3, tags=["tailscale"])

_add(kind="rule", title="Never expose services without tailscale",
    body="Home services must only bind to the tailscale interface, not 0.0.0.0. Audit with ss -tulpn monthly.",
    area="infra", importance=5, tags=["security", "rule"])

_add(kind="preference", title="Infra config in git",
    body="All home-lab configs (docker-compose, tailscale rules, launchd plists) live in a private git repo. Never edit in place on the host.",
    area="infra", importance=3, tags=["ops"])

_add(kind="note", title="Kai upgraded Home Assistant",
    body="Kai upgraded Home Assistant from 2024.2 to 2024.4. One broken integration (z2m) needed a manual reconfigure. All zone automations still work.",
    area="infra", importance=2, tags=["homeassistant"])

_add(kind="fact", title="Backup strategy",
    body="server-mini backs up config and HA data nightly via rclone to encrypted S3 bucket. Retention 30 days. Restore tested quarterly.",
    area="infra", importance=4, tags=["backup"])

_add(kind="learning", title="Docker compose env file precedence",
    body="docker-compose picks up .env from the compose file's directory, not the current working directory. Was debugging for an hour why env vars weren't loading.",
    area="infra", importance=3, tags=["docker"])

# SUPERSEDED — old exit node was desktop-gpu
_add(kind="fact", title="Old tailscale exit node",
    body="Exit node used to be desktop-gpu during 2023. Moved to server-mini because desktop-gpu was off overnight and broke the home-VPN use case.",
    area="infra", importance=1, tags=["tailscale", "history", "outdated"])
