---
plan_id: pl_dd93d764e1b94293b5e943af71dfdb5b
kind: task
id: task_3c9ffa8ccbec4e4b9d7754e5e1620e05
handle: gh-188-macos-blank-window-after-wkwebview-webcontent-reap-add-reload-han
title: 'GH #188: macOS blank window after WKWebView WebContent reap — add reload handler'
status: done
---

STEP 1: gh issue view 188 — issue body is the spec. When macOS reaps the WKWebView WebContent process (memory pressure / sleep), the native window stays permanently blank; there is no reload/recovery handler. Fix: handle the WebContent-process-terminated callback in the Tauri/wry layer or via Rust-side window event (webview_process_did_terminate / render-process-gone equivalent) and reload the webview automatically, restoring the app shell. Repro hint: kill the WebContent process manually (pkill -f 'WebKit.WebContent' scoped to the dev app) and observe the blank window. Acceptance: after WebContent kill the UI self-recovers within 3s without user action, session state intact (panes reattach).
