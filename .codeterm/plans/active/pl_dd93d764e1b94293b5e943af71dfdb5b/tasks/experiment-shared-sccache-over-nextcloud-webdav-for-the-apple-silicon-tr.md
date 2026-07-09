---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_fe8b4bd1278d48e1b5acd3d7ad710dee
handle: experiment-shared-sccache-over-nextcloud-webdav-for-the-apple-silicon-tr
title: 'Experiment: shared sccache over Nextcloud WebDAV for the Apple Silicon trio'
status: cancelled
---

User idea (round-17): shared compile cache via our existing Nextcloud WebDAV (ainyiva-bot, per-peer app tokens already provisioned). Scope honestly bounded by cache-key reality: sccache keys include the target triple, so the 4-machine release farm gains ~nothing (all different triples; local persistent runner disks already cover repeats). The REAL candidate is the aarch64-darwin trio: Mac-Air (primes daily) <-> eight (cold Dali lanes) <-> Mac builder. Caveat to verify: with a webdav backend sccache sends ALL lookups to the remote (no local tier) — may hurt interactive dev even while helping cold lanes. EXPERIMENT (one pair first, eight pulls Mac-Air-primed cache): (1) create a dedicated Nextcloud folder + app token; configure sccache webdav on EIGHT only (SCCACHE_WEBDAV_ENDPOINT/TOKEN via ~/.cargo/config.toml env or wrapper script); Mac-Air keeps local disk but uploads via a one-off primed push run OR flip Mac-Air to webdav for one priming build. (2) Measure on eight, 3 runs median each: cold worktree 'cargo check --manifest-path src-tauri/Cargo.toml' with (a) local-only sccache primed, (b) webdav-shared cache, (c) no cache baseline. (3) Also measure one interactive incremental check on eight under webdav to quantify the no-local-tier penalty. (4) Decision by numbers recorded as a working point: adopt for eight lanes / adopt for release Mac builder only / drop. Coordinate with Dali via mesh — he runs the eight side.
