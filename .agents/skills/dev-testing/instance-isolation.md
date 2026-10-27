# Development instance paths

Repository development builds use the following isolation. Read the dev-run
skill for process identity and safe lifecycle commands.

## Dev/Prod Isolation

Dev builds use identifier `com.domios.dev` and are fully isolated from production:
- **API port:** 7686 (prod: 7685)
- **Data directory:** `~/.domios-dev/` (prod: `~/.domios/`)
- **Socket prefix:** `domios-dev` (prod: `domios`)
- **Single-instance:** Disabled in dev builds, so dev and prod can run side by side.


## Named instance connections

`CODETERM_INSTANCE` names a development instance. Pair it with the matching
API port; a conflicting default port or socket is refused.

Development builds (the default DEV instance or a named one) may explicitly set `api.mesh_tailnet_host` to a locally assigned Tailscale IPv4 address. This adds an authenticated listener while retaining the loopback primary; the default is absent. Production builds refuse this option. A restart is required. Both sockets must bind and pass health checks before remote readiness is published; startup never falls back to a LAN or wildcard bind. If the extra listener exits after readiness, loopback remains serving, remote readiness is withdrawn, and broadcasting stops. `api_listener.ready_addresses` is the live health-proven set, not the configured request; `requested_mesh_tailnet_address` records the startup request only.
