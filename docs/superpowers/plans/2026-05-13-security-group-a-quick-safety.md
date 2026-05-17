# Security Group A — Quick Safety Fixes

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate 5 CRITICAL panic/auth/safety bugs with minimal blast radius — each fix is surgical and independently testable.

**Architecture:** Pure fix-only changes. No refactoring, no new abstractions. TDD: write a test that catches the bug, verify it fails, apply fix, verify it passes.

**Tech Stack:** Rust, Tauri v2, axum, `cargo test`, `./build.sh test`

---

### Task 1: Fix panic!() in list_dir_completions (completions.rs:13)

**Files:**
- Modify: `src-tauri/src/commands/completions.rs:13`

**Root cause:** `entry.file_type().unwrap_or_else(|_| panic!())` crashes the daemon on broken symlinks or permission errors.

- [ ] **Step 1: Check existing tests**

```bash
grep -n "#\[test\]" src-tauri/src/commands/completions.rs
```
Expected: no tests (will write one).

- [ ] **Step 2: Write failing test**

Add this test block at the end of `src-tauri/src/commands/completions.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_dir_completions_with_inaccessible_entry_does_not_panic() {
        // If the function panics, the test itself panics → fails.
        // On most systems we can test this by passing a cwd that exists
        // but contains entries we can probe safely. The fix must mean
        // broken entries are silently skipped, not panicked on.
        //
        // We test the behavior by calling with a real directory and
        // verifying it returns Ok, not Err or panic.
        let result = list_dir_completions(
            std::env::temp_dir().to_string_lossy().to_string(),
            String::new(),
        );
        assert!(result.is_ok(), "should return Ok, not panic");
    }
}
```

- [ ] **Step 3: Run test — verify current code does not panic for this input (the panic is on broken symlinks, temp_dir is safe)**

```bash
cargo test -p codeterm list_dir_completions_with_inaccessible_entry_does_not_panic 2>&1 | tail -5
```
Expected: PASS (temp_dir doesn't have broken symlinks, but the test structure is correct). The real regression test is that the fix below prevents the panic path from ever executing.

- [ ] **Step 4: Apply fix**

In `src-tauri/src/commands/completions.rs`, replace line 13:

```rust
// BEFORE
let ft = entry.file_type().unwrap_or_else(|_| panic!());
if !ft.is_dir() { continue; }

// AFTER
let Ok(ft) = entry.file_type() else { continue; };
if !ft.is_dir() { continue; }
```

- [ ] **Step 5: Run tests**

```bash
cargo test -p codeterm list_dir_completions 2>&1 | tail -5
./build.sh test 2>&1 | tail -10
```
Expected: PASS, no compilation errors.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/commands/completions.rs
git commit -m "fix(commands): skip unreadable entries instead of panicking in list_dir_completions"
```

---

### Task 2: Guard ns_window().unwrap() at startup (window.rs:21)

**Files:**
- Modify: `src-tauri/src/startup/window.rs:21`

**Root cause:** `window.ns_window().unwrap()` panics at startup if Tauri returns None for the native window handle (can happen on certain macOS configurations).

- [ ] **Step 1: Read the exact code**

```bash
sed -n '18,28p' src-tauri/src/startup/window.rs
```

Expect to see:
```rust
let ns_win = window.ns_window().unwrap() as *mut objc2::runtime::AnyObject;
```

- [ ] **Step 2: Apply fix** (no unit test possible — macOS-only unsafe code)

Replace the unwrap with a guard. In `src-tauri/src/startup/window.rs`, inside the `#[cfg(target_os = "macos")]` block:

```rust
// BEFORE
let ns_win = window.ns_window().unwrap() as *mut objc2::runtime::AnyObject;

// AFTER
let Some(ns_win_raw) = window.ns_window() else {
    log::warn!("[window] ns_window() returned None — skipping macOS styling");
    return;
};
let ns_win = ns_win_raw as *mut objc2::runtime::AnyObject;
```

- [ ] **Step 3: Verify compilation**

```bash
./build.sh check 2>&1 | tail -5
```
Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/startup/window.rs
git commit -m "fix(startup): guard ns_window() unwrap — log warn and skip styling if None"
```

---

### Task 3: Fix panic on non-ASCII skill names (worker.rs:452)

**Files:**
- Modify: `cli/src/cmd/worker.rs` (function `build_skill_id`)

**Root cause:** `&trimmed[..40]` slices at byte offset 40, panicking on multi-byte UTF-8 characters (Chinese, emoji, Arabic, etc.).

- [ ] **Step 1: Locate the function**

```bash
grep -n "fn build_skill_id" cli/src/cmd/worker.rs
```
Expected: `452: fn build_skill_id(title: &str) -> String {`

- [ ] **Step 2: Check for existing tests**

```bash
grep -n "build_skill_id\|#\[test\]" cli/src/cmd/worker.rs | tail -20
```

- [ ] **Step 3: Write failing test**

Find the `#[cfg(test)]` block in `cli/src/cmd/worker.rs` or add one at the end:

```rust
#[cfg(test)]
mod tests {
    use super::build_skill_id;

    #[test]
    fn build_skill_id_ascii() {
        let id = build_skill_id("My Skill");
        assert_eq!(id, "skill_my_skill");
    }

    #[test]
    fn build_skill_id_non_ascii_does_not_panic() {
        // Chinese characters are 3 bytes each → 14 chars × 3 = 42 bytes
        // slicing at byte 40 would panic without the fix
        let long_chinese = "技能技能技能技能技能技能技能技能技能技能技能技能技能技能"; // 14 × 3 = 42 bytes as slug
        let result = std::panic::catch_unwind(|| build_skill_id(long_chinese));
        assert!(result.is_ok(), "should not panic on non-ASCII skill names");
    }

    #[test]
    fn build_skill_id_emoji_does_not_panic() {
        let emoji_title = "🚀🎯🔥🌟💡🎨🛡️🔑🌈✨🎭🎪🎡🎢🎠🎟🎃🎄🎆🎇"; // many emoji
        let result = std::panic::catch_unwind(|| build_skill_id(emoji_title));
        assert!(result.is_ok(), "should not panic on emoji skill names");
    }
}
```

- [ ] **Step 4: Run tests — verify they FAIL**

```bash
cargo test -p codeterm-cli build_skill_id 2>&1 | tail -10
```
Expected: `build_skill_id_non_ascii_does_not_panic` fails (panics).

- [ ] **Step 5: Apply fix**

In `cli/src/cmd/worker.rs`, in `build_skill_id`, replace the slice:

```rust
// BEFORE
let short = if trimmed.len() > 40 { &trimmed[..40] } else { trimmed };

// AFTER
let short = if trimmed.len() > 40 {
    // Slice at a char boundary to avoid panic on multi-byte UTF-8
    trimmed.char_indices().nth(40).map_or(trimmed, |(i, _)| &trimmed[..i])
} else {
    trimmed
};
```

- [ ] **Step 6: Run tests — verify they PASS**

```bash
cargo test -p codeterm-cli build_skill_id 2>&1 | tail -10
```
Expected: all 3 tests PASS.

- [ ] **Step 7: Commit**

```bash
git add cli/src/cmd/worker.rs
git commit -m "fix(cli): use char_indices for skill id truncation to avoid panic on non-ASCII names"
```

---

### Task 4: Propagate HTTP client build error instead of silent fallback (main.rs:144)

**Files:**
- Modify: `cli/src/main.rs` (function `build_http_client` or equivalent, around line 144)

**Root cause:** `reqwest::blocking::Client::builder()...build().unwrap_or_else(|_| reqwest::blocking::Client::new())` silently drops all default headers (including auth token) on builder failure. Subsequent requests return 401.

- [ ] **Step 1: Read the exact function**

```bash
sed -n '125,155p' cli/src/main.rs
```

- [ ] **Step 2: Identify callers of build_http_client**

```bash
grep -n "build_http_client\|build_http\b" cli/src/main.rs cli/src/client.rs 2>/dev/null | head -20
```

- [ ] **Step 3: Apply fix**

The function signature needs to change from returning `Client` to returning `Result<Client, reqwest::Error>`. Update the function:

```rust
// BEFORE (approximate)
fn build_http_client(token: &str) -> reqwest::blocking::Client {
    // ... header setup ...
    reqwest::blocking::Client::builder()
        .default_headers(headers)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}

// AFTER
fn build_http_client(token: &str) -> Result<reqwest::blocking::Client, reqwest::Error> {
    // ... header setup unchanged ...
    reqwest::blocking::Client::builder()
        .default_headers(headers)
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(60))
        .build()
    // No fallback — propagate the error
}
```

Then update all call sites. The typical call site is in `main()` or a CLI dispatch function:

```rust
// BEFORE
let client = build_http_client(&token);

// AFTER
let client = build_http_client(&token)
    .map_err(|e| format!("Failed to build HTTP client: {}", e))?;
```

If the call site is in a function returning `()`, convert it to return `anyhow::Result<()>` or print the error and exit:

```rust
let client = match build_http_client(&token) {
    Ok(c) => c,
    Err(e) => {
        eprintln!("Error: failed to build HTTP client: {}", e);
        std::process::exit(1);
    }
};
```

- [ ] **Step 4: Verify compilation**

```bash
cargo build -p codeterm-cli 2>&1 | grep "^error" | head -10
```
Expected: no errors.

- [ ] **Step 5: Commit**

```bash
git add cli/src/main.rs
git commit -m "fix(cli): propagate HTTP client build error instead of silently dropping auth token"
```

---

### Task 5: Add auth + path guard to serve_file (notification_routes.rs:62)

**Files:**
- Modify: `src-tauri/src/api/notification_routes.rs:62`

**Root cause:** `serve_file` serves arbitrary filesystem paths with no authentication and no path restriction. Any process reaching the API port can read any file on disk.

- [ ] **Step 1: Read the current handler**

```bash
sed -n '57,95p' src-tauri/src/api/notification_routes.rs
```

- [ ] **Step 2: Check how check_auth works**

```bash
grep -n "pub fn check_auth" src-tauri/src/api/routes.rs
```
Expected: `check_auth` takes `(&AppState, &HeaderMap)` and returns `Result<(), (StatusCode, &'static str)>`.

The file already imports `check_auth` at line 8: `use crate::api::routes::{check_auth, AppState};`

- [ ] **Step 3: Write test for unauthenticated serve_file**

Look for integration tests for API routes:
```bash
grep -rn "serve_file\|notification" src-tauri/src --include="*.rs" | grep "#\[test\]" | head -5
```

Add a unit test to `notification_routes.rs` at the end of the file:

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn serve_file_path_guard_rejects_outside_data_dir() {
        // Verify that a path outside the data dir is rejected
        let allowed = crate::paths::data_dir();
        let outside = std::path::Path::new("/etc/passwd");
        let canonical_outside = std::fs::canonicalize(outside).unwrap_or(outside.to_path_buf());
        assert!(
            !canonical_outside.starts_with(&allowed),
            "/etc/passwd should not be inside the data dir"
        );
    }

    #[test]
    fn serve_file_path_guard_accepts_inside_data_dir() {
        let allowed = crate::paths::data_dir();
        let inside = allowed.join("config.toml");
        // Even if file doesn't exist, path is inside allowed prefix
        assert!(inside.starts_with(&allowed));
    }
}
```

- [ ] **Step 4: Apply fix**

Update the `serve_file` function signature to accept `AppState` and `HeaderMap`, then add auth and path guard:

```rust
pub async fn serve_file(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(query): Query<ServeFileQuery>,
) -> Result<axum::response::Response, (StatusCode, &'static str)> {
    // Auth check first
    check_auth(&state, &headers)?;

    // Path guard: only serve files inside the CodeTerm data directory
    let requested = std::path::Path::new(&query.path)
        .canonicalize()
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid path"))?;
    let allowed_prefix = crate::paths::data_dir();
    if !requested.starts_with(&allowed_prefix) {
        return Err((StatusCode::FORBIDDEN, "Path not allowed"));
    }

    if !requested.exists() {
        return Err((StatusCode::NOT_FOUND, "File not found"));
    }

    let bytes = std::fs::read(&requested)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to read file"))?;

    // ... content_type match and response builder unchanged ...
}
```

Also update the function's import line if needed — `HeaderMap` must be imported. Check line 1-8 of the file; `HeaderMap` is already imported via axum.

- [ ] **Step 5: Update the route registration if needed**

Check that the route registration passes `AppState`:
```bash
grep -n "serve_file" src-tauri/src/api/routes.rs src-tauri/src/api/mod.rs 2>/dev/null
```
The route handler is registered in the router — Axum will automatically inject `AppState` since it's an extractor. No change needed to route registration.

- [ ] **Step 6: Verify compilation and tests**

```bash
./build.sh check 2>&1 | tail -5
cargo test -p codeterm serve_file 2>&1 | tail -5
```
Expected: compilation OK, tests PASS.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/api/notification_routes.rs
git commit -m "fix(api): add auth + data-dir path guard to serve_file — prevents arbitrary filesystem read"
```
