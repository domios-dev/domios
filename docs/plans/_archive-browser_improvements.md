# Browser Improvements Plan

> **Status (2026-04-13):** ~20% done. Skeleton routes + pane type exist. P0 (3rd-party cookies), P1 (address bar/nav), P2 (screenshots) pending.

## Current State

- Browser panes use Tauri's `WebviewWindow` (WebView2 on Windows, WKWebView on macOS)
- Basic URL loading, no address bar, no navigation controls
- 3rd party cookies blocked by default (WebView2 Tracking Prevention = "Balanced")
- Agents can open browser panels via `POST /api/browser/open` or `codeterm browser open`
- No agent control of browser content (no CDP, no screenshot of web pages)

## Problems to Solve

### P0: 3rd Party Cookies (Login Providers Broken)
- **Issue**: Clerk, PropelAuth, Auth0 and similar OAuth providers use 3rd party cookies. WebView2 blocks them, making login flows fail silently.
- **Fix — Windows (WebView2)**: Disable Tracking Prevention via environment options:
  - Option A: `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--disable-features=ThirdPartyCookieBlocking` (env var, set before webview creation)
  - Option B: Add `additionalBrowserArgs` in `tauri.conf.json` (but careful — bug #11144, all webviews must share same args)
  - Option C: wry-level `CoreWebView2EnvironmentOptions::EnableTrackingPrevention(false)` — most reliable
  - **Caveat**: Tauri bug #13092 — additional WebviewWindows can show blank when `additionalBrowserArgs` set. May need to create browser webviews differently.
- **Fix — macOS (WKWebView)**: Generally works, but ITP can interfere. May need `WKWebsiteDataStore` configuration.
- **Fix — Linux (WebKitGTK)**: Generally works out of the box.
- **Config**: Add `browser.allow_third_party_cookies: bool` (default: true) to AppConfig

### P1: Browser Settings
- **Address bar + navigation** — back/forward/reload/URL input bar above browser pane
- **DevTools** — toggle button or keyboard shortcut to open DevTools for the browser pane
- **User-agent override** — useful for mobile testing
- **JavaScript toggle** — disable JS for specific pages
- **Config section**: `[browser]` in config.toml:
  ```toml
  [browser]
  allow_third_party_cookies = true
  default_url = "about:blank"
  user_agent = ""  # empty = default
  ```

### P2: Agent Browser Control (Inspired by OpenClaw)
OpenClaw uses Chrome DevTools Protocol (CDP) for full browser control. For CodeTerm, a lighter approach:

#### Phase 1: Screenshot + Basic Navigation
- `GET /api/panes/{id}/browser/screenshot` — capture browser pane content as JPEG
- `POST /api/panes/{id}/browser/navigate` — `{url: "..."}`
- `POST /api/panes/{id}/browser/back` / `forward` / `reload`
- `GET /api/panes/{id}/browser/url` — current URL
- CLI: `codeterm browser screenshot --pane ID --output file.jpg`

#### Phase 2: DOM Interaction (CDP-lite)
- `POST /api/panes/{id}/browser/eval` — execute JavaScript, return result
- `POST /api/panes/{id}/browser/click` — click at coordinates or CSS selector
- `POST /api/panes/{id}/browser/type` — type text into focused element
- `GET /api/panes/{id}/browser/html` — get page HTML (or selected element)

#### Phase 3: Full CDP (if needed)
- Expose CDP WebSocket endpoint for browser panes
- Agents connect directly to CDP for Playwright-like control
- Profile isolation: separate browser data directory per workspace

### P3: Pane Type Conversion
- Convert terminal pane → browser pane (and back)
- Already have the globe button in PaneToolbar — it should replace the current pane, not create a new split
- Need `POST /api/panes/{id}/convert` endpoint or just close + create in same position

## Implementation Order

1. **3rd party cookies fix** — highest user impact, relatively simple
2. **Browser config section** — `[browser]` in config.toml + settings UI tab
3. **Address bar + navigation** — browser pane chrome (back/forward/url/reload)
4. **Agent screenshot of browser** — `browser/screenshot` endpoint
5. **Agent navigation** — `browser/navigate`, `browser/url` endpoints
6. **DOM interaction** — eval, click, type (lower priority)
7. **CDP passthrough** — only if there's demand

## References

- [OpenClaw Browser Docs](https://docs.openclaw.ai/tools/browser) — CDP architecture, snapshot-based refs
- [OpenClaw Browser Relay](https://www.aifreeapi.com/en/posts/openclaw-browser-relay-guide) — Chrome extension bridge pattern
- [Tauri WebView2 cookies bug #11144](https://github.com/tauri-apps/tauri/issues/11144)
- [Tauri additionalBrowserArgs bug #13092](https://github.com/tauri-apps/tauri/issues/13092)
- [WebView2 3rd party cookies #1665](https://github.com/MicrosoftEdge/WebView2Feedback/issues/1665)
- [WebView2 TrackingPrevention #2013](https://github.com/MicrosoftEdge/WebView2Feedback/issues/2013)
- [openclaw-claude-code-skill](https://github.com/Enderfga/openclaw-claude-code-skill) — MCP integration pattern
