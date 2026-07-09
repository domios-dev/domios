---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: milestone
id: ms_d7caf0ca7f2a4752bce4df00e00f1272
handle: responsiveness-soak-no-ui-hitches-under-real-load-web-native-ms_d7caf0ca7f2a4752bce4df00e00f1272
title: status_update
---

Accepted native/WebView2 responsiveness soak. Harness integrated at b729306d as tests/playwright/soak/responsiveness-soak-native.cjs after trimming comments to repo discipline. ROG run: Windows 11 Home 10.0.26200, WebView2 CDP, exact tested app SHA fc5e6c3e, full 300000ms soak with durationMs 300439, frames 143810, iterations 116, maxRafGapMs 25, rafGapsOver250 0, longtasksOver250 0, longtaskCount 0, window.__ctPerf SLOW-IPC 0, dev-log SLOW-IPC 0. Dev-log LOCK-HOLD count 2 during setup before full soak (mux.read 514ms, pty.lock 480ms), no soak-phase hitch. Busy evidence captured via statusChip=true and tabStatusDot=true; artifacts on ROG at C:/w-soak/artifacts/native-soak-full/. Follow-up noted but non-blocking: Windows synthetic plan workspace matching with \?\ path prefix forced titlebar canvas fallback in harness.
