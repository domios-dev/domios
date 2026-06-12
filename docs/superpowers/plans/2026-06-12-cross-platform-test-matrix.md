# Cross-Platform Test Matrix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **Commit policy:** Per project rules, NEVER commit or push without an explicit user request. Tasks below end with a "checkpoint" step (verify + report) instead of a commit step. When the user says "commit", commit all completed tasks at once, no Co-Authored-By lines.

**Goal:** Run the full unit-test suite for Linux, Windows, and macOS in parallel — Linux isolated in Docker on this Mac, Windows compile-verified in Docker locally and fully executed in GitHub Actions, macOS natively on the host — all from one command.

**Architecture:** Three local "lanes" (macOS native, Linux Docker, Windows cross-compile check) orchestrated by `scripts/test-matrix.sh` running them concurrently with per-lane logs and a summary table. Docker containers mount the repo and shadow mutable dirs (`src-tauri/target`, `node_modules`) with named volumes so host artifacts never mix with container artifacts and the host repo stays clean. A new CI workflow `test.yml` runs the same unit suites on real ubuntu-24.04 / windows-latest / macos-latest runners — this is the only place Windows tests actually *execute* in isolation (Windows containers cannot run on a macOS Docker host; ConPTY does not exist under Wine).

**Tech Stack:** Docker (linux/arm64 on Apple Silicon), `cargo-xwin` (MSVC cross-compilation from Linux), GitHub Actions, bash.

**Hard platform facts this plan is built on:**
- Docker on macOS runs only Linux containers. Windows containers require a Windows host; macOS containers do not exist. So "all three OSes in Docker on this machine" is physically impossible — this plan gets the closest realistic equivalent.
- ubuntu-24.04 base image is mandatory for Linux: the pyke/ort prebuilt onnxruntime needs glibc >= 2.38 (see header comment in `.github/workflows/release-linux.yml`).
- Host is arm64 → the Linux container is aarch64-unknown-linux-gnu, which is a real release-adjacent target and runs at native speed. If the ort prebuilt for linux-aarch64 fails to link, fallback is `--platform linux/amd64` (Rosetta emulation, slower).
- `cargo check --target x86_64-pc-windows-msvc` compiles all Windows-only code paths (`#[cfg(windows)]`, `windows` crate, ConPTY glue) without linking, so it does not need the onnxruntime Windows `.lib` — that's why the local Windows lane is `check`, not `test`.

**Out of scope (deliberate, follow-up plans if wanted):**
- E2E (`pytest tests/e2e/`) inside the Linux container — needs the Tauri app running under xvfb; separate plan once the unit matrix is in place.
- macOS *isolation* via Tart VMs — native host run already provides macOS coverage; isolation adds cost without test value today (YAGNI).
- Running Windows test binaries under Wine — ConPTY-dependent tests can't work; real execution is covered by CI and/or a real Windows machine on the CodeTerm mesh (cross-compiled binaries built here, test job dispatched to the peer; machine names deliberately not recorded in repo files).

**File map:**

| File | Responsibility |
|---|---|
| `docker/test-linux.dockerfile` | Create: Linux unit-test image (Rust + Node + Tauri system deps) |
| `docker/test-windows-cross.dockerfile` | Create: cargo-xwin image for Windows compile check |
| `scripts/test-macos-native.sh` | Create: macOS lane (host cargo + tsx tests) |
| `scripts/test-linux-docker.sh` | Create: Linux lane (build image, run tests in container) |
| `scripts/test-windows-cross.sh` | Create: Windows lane (cargo xwin check in container) |
| `scripts/test-matrix.sh` | Create: parallel orchestrator + summary |
| `.github/workflows/test.yml` | Create: 3-OS CI unit-test matrix (real Windows execution) |
| `docs/TESTING.md` | Modify: new "Cross-platform test matrix" section, bump revision |

---

### Task 1: Linux test image

**Files:**
- Create: `docker/test-linux.dockerfile`

- [ ] **Step 1: Write the Dockerfile**

```dockerfile
# Unit-test image for the Linux lane of scripts/test-matrix.sh.
# ubuntu:24.04 is required: pyke/ort prebuilt onnxruntime needs glibc >= 2.38.
FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git build-essential pkg-config \
    libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
    librsvg2-dev libssl-dev patchelf \
 && curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
 && apt-get install -y --no-install-recommends nodejs \
 && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain stable --profile minimal
ENV PATH="/root/.cargo/bin:${PATH}"

RUN npm install -g pnpm@10.32.1

WORKDIR /work
```

The apt package list is copied verbatim from `.github/workflows/release-linux.yml` (the known-good set for building src-tauri on Linux), plus toolchain basics. pnpm version 10.32.1 matches the release workflows.

- [ ] **Step 2: Build the image and verify**

Run: `docker build -t codeterm-test-linux -f docker/test-linux.dockerfile docker/`
Expected: build completes; final line `naming to docker.io/library/codeterm-test-linux`.

Run: `docker run --rm codeterm-test-linux bash -c 'rustc --version && node --version && pnpm --version'`
Expected: three version lines (rustc 1.x stable, node v20.x, pnpm 10.32.1), exit 0.

- [ ] **Step 3: Checkpoint**

Report: image name, image size (`docker images codeterm-test-linux`), build time. Do not commit.

---

### Task 2: Linux lane script

**Files:**
- Create: `scripts/test-linux-docker.sh`

- [ ] **Step 1: Write the lane script**

```bash
#!/usr/bin/env bash
# Linux lane: Rust + TS unit tests inside a Docker container.
# The repo is bind-mounted; src-tauri/target and node_modules are shadowed
# by named volumes so Linux artifacts never touch the host checkout.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
IMAGE=codeterm-test-linux

docker build -q -t "$IMAGE" -f "$ROOT/docker/test-linux.dockerfile" "$ROOT/docker" >/dev/null

docker run --rm \
  -v "$ROOT:/work" \
  -v codeterm-linux-cargo-registry:/root/.cargo/registry \
  -v codeterm-linux-target:/work/src-tauri/target \
  -v codeterm-linux-node-modules:/work/node_modules \
  -w /work \
  "$IMAGE" bash -c '
    set -euo pipefail
    mkdir -p dist
    echo "=== Rust unit tests (linux, $(uname -m)) ==="
    cargo test --manifest-path src-tauri/Cargo.toml --lib
    echo "=== TypeScript unit tests (linux) ==="
    pnpm install --frozen-lockfile
    npx tsx src/components/ChatView/parser.test.ts
  '
```

`mkdir -p dist` exists because tauri-build's build script expects the `frontendDist` directory to exist; on a fresh container-eyes view of the repo it may not. The three named volumes persist between runs — first run downloads crates and compiles from scratch, subsequent runs are incremental.

- [ ] **Step 2: Make it executable**

Run: `chmod +x scripts/test-linux-docker.sh`

- [ ] **Step 3: First (cold) run to verify**

Run: `./scripts/test-linux-docker.sh`
Expected: cold run compiles everything (this is the slow one-time cost), then `test result: ok. 210 passed` (count may have drifted upward — any number with `0 failed` is a pass) and the TS suite's pass output, exit 0.

If the run fails at the **link** step with onnxruntime/ort symbols missing for aarch64: edit the `docker run` line to add `--platform linux/amd64` and the `docker build` to match, rerun (Rosetta-emulated, slower, but unblocks). Record which mode was needed.

- [ ] **Step 4: Warm-run sanity**

Run: `./scripts/test-linux-docker.sh` again.
Expected: same pass output, dramatically faster (incremental compile via the `codeterm-linux-target` volume).

- [ ] **Step 5: Verify host checkout untouched**

Run: `git status --porcelain`
Expected: only this plan's new files; no stray `node_modules`/`target` changes from the container run.

- [ ] **Step 6: Checkpoint**

Report cold and warm run wall-clock times. Do not commit.

---

### Task 3: Windows cross-compile check lane

**Files:**
- Create: `docker/test-windows-cross.dockerfile`
- Create: `scripts/test-windows-cross.sh`

- [ ] **Step 1: Write the Dockerfile**

```dockerfile
# Windows compile-check image: cargo-xwin cross-compiles src-tauri to
# x86_64-pc-windows-msvc from Linux. `check` only — no link, no test run.
# Real Windows test execution lives in .github/workflows/test.yml (windows-latest).
FROM rust:bookworm

RUN apt-get update && apt-get install -y --no-install-recommends \
    clang lld llvm \
 && rm -rf /var/lib/apt/lists/*

RUN rustup target add x86_64-pc-windows-msvc \
 && cargo install --locked cargo-xwin

WORKDIR /work
```

clang/lld/llvm are hard requirements of cargo-xwin (it uses clang-cl semantics and llvm-lib/lld-link for the MSVC ABI).

- [ ] **Step 2: Write the lane script**

```bash
#!/usr/bin/env bash
# Windows lane: cross-compile check against x86_64-pc-windows-msvc.
# Catches #[cfg(windows)] compile breakage locally in minutes without a
# Windows machine. Does NOT execute tests — see .github/workflows/test.yml.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
IMAGE=codeterm-test-windows-cross

docker build -q -t "$IMAGE" -f "$ROOT/docker/test-windows-cross.dockerfile" "$ROOT/docker" >/dev/null

docker run --rm \
  -v "$ROOT:/work" \
  -v codeterm-xwin-cargo-registry:/usr/local/cargo/registry \
  -v codeterm-xwin-cache:/root/.cache/cargo-xwin \
  -v codeterm-xwin-target:/work/src-tauri/target \
  -e XWIN_ACCEPT_LICENSE=1 \
  -w /work \
  "$IMAGE" bash -c '
    set -euo pipefail
    mkdir -p dist
    echo "=== cargo check x86_64-pc-windows-msvc ==="
    cargo xwin check --target x86_64-pc-windows-msvc \
      --manifest-path src-tauri/Cargo.toml --lib --tests
  '
```

Notes: the `rust` official image keeps cargo home at `/usr/local/cargo` (different from Task 2's `/root/.cargo`). The `codeterm-xwin-cache` volume keeps the downloaded MSVC CRT + Windows SDK (~1 GB, one-time). `--tests` makes test code compile too, not just the lib. `XWIN_ACCEPT_LICENSE=1` accepts the Microsoft SDK license non-interactively.

- [ ] **Step 3: Make it executable**

Run: `chmod +x scripts/test-windows-cross.sh`

- [ ] **Step 4: Run and verify**

Run: `./scripts/test-windows-cross.sh`
Expected: first run downloads the SDK then type-checks the full crate for the Windows target; ends with `Finished` and exit 0.

If this surfaces *real* Windows compile errors in current code — that is the lane working as designed. Report them to the user as findings; do not silently patch unrelated source files as part of this plan.

If cargo-xwin itself cannot handle a build-script dependency (known rough edge: crates whose build.rs probes Windows-only tooling), record the exact error and report — fallback is dropping this lane and relying on CI `windows-latest` for compile coverage (Task 5 works without this lane).

- [ ] **Step 5: Checkpoint**

Report run time and any Windows-target findings. Do not commit.

---

### Task 4: macOS native lane

**Files:**
- Create: `scripts/test-macos-native.sh`

- [ ] **Step 1: Write the lane script**

```bash
#!/usr/bin/env bash
# macOS lane: native host run. macOS cannot be containerized; the host
# IS the macOS test environment. Mirrors ./build.sh test + TS tests.
set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

echo "=== Rust unit tests (macos, $(uname -m)) ==="
cargo test --manifest-path src-tauri/Cargo.toml --lib

echo "=== TypeScript unit tests (macos) ==="
npx tsx src/components/ChatView/parser.test.ts
```

- [ ] **Step 2: Make it executable and run**

Run: `chmod +x scripts/test-macos-native.sh && ./scripts/test-macos-native.sh`
Expected: Rust `test result: ok` with `0 failed`, TS suite passes, exit 0.

- [ ] **Step 3: Checkpoint**

Do not commit.

---

### Task 5: Parallel orchestrator

**Files:**
- Create: `scripts/test-matrix.sh`

- [ ] **Step 1: Write the orchestrator**

```bash
#!/usr/bin/env bash
# Cross-platform test matrix: runs all lanes concurrently.
#   macos          — native host (Rust + TS unit tests)
#   linux          — Docker container (Rust + TS unit tests)
#   windows-cross  — Docker container (cargo check vs x86_64-pc-windows-msvc)
# Full Windows test EXECUTION is CI-only: .github/workflows/test.yml.
# Usage: scripts/test-matrix.sh [lane...]   (default: all lanes)
set -uo pipefail

ROOT="$(git rev-parse --show-toplevel)"
LOGS="${TMPDIR:-/tmp}/codeterm-test-matrix"
mkdir -p "$LOGS"

# No associative arrays: macOS ships bash 3.2 and /usr/bin/env bash may
# resolve to it — keep the script 3.2-compatible.
lane_script() {
  case "$1" in
    macos)         echo "$ROOT/scripts/test-macos-native.sh" ;;
    linux)         echo "$ROOT/scripts/test-linux-docker.sh" ;;
    windows-cross) echo "$ROOT/scripts/test-windows-cross.sh" ;;
    *)             return 1 ;;
  esac
}

if [ $# -gt 0 ]; then LANES=("$@"); else LANES=(macos linux windows-cross); fi

PIDS=()
START=$SECONDS
for lane in "${LANES[@]}"; do
  script="$(lane_script "$lane")" || { echo "unknown lane: $lane" >&2; exit 2; }
  "$script" >"$LOGS/$lane.log" 2>&1 &
  PIDS+=($!)
  echo "started  $lane (pid $!, log $LOGS/$lane.log)"
done

FAIL=0
i=0
for lane in "${LANES[@]}"; do
  if wait "${PIDS[$i]}"; then
    echo "PASS  $lane"
  else
    echo "FAIL  $lane — tail of log:"
    tail -n 20 "$LOGS/$lane.log" | sed 's/^/      /'
    FAIL=1
  fi
  i=$((i + 1))
done

echo "total: $((SECONDS - START))s"
exit $FAIL
```

- [ ] **Step 2: Make it executable**

Run: `chmod +x scripts/test-matrix.sh`

- [ ] **Step 3: Verify failure reporting (negative test first)**

Run: `bash -c 'scripts/test-matrix.sh nosuchlane; echo "exit=$?"'`
Expected: `unknown lane: nosuchlane`, `exit=2`.

Then verify a real lane failure propagates: run with a PATH that keeps bash/git but hides cargo (`~/.cargo/bin`): `PATH=/usr/bin:/bin scripts/test-matrix.sh macos`.
Expected: `FAIL  macos` with log tail (cargo: command not found), exit 1.

- [ ] **Step 4: Full parallel run**

Run: `./scripts/test-matrix.sh`
Expected: three `started` lines immediately (proof of parallelism), then `PASS macos`, `PASS linux`, `PASS windows-cross`, total time ≈ the slowest lane (not the sum), exit 0.

- [ ] **Step 5: Checkpoint**

Report the summary table and total wall-clock. Do not commit.

---

### Task 6: CI workflow — real 3-OS execution

> **DEFERRED (user decision 2026-06-12):** no GitHub-hosted test runs for now — runner minutes are too slow/expensive for this repo. Real Windows execution instead goes through a Windows mesh peer: `cargo xwin test --no-run` builds Windows test binaries locally, the mesh peer executes them (follow-up plan). The workflow below is kept for reference if CI is revisited.

**Files:**
- Create: `.github/workflows/test.yml`

This is the only place Windows unit tests actually *run* in an isolated environment. Mirrors the toolchain setup of the existing release workflows.

- [ ] **Step 1: Write the workflow**

```yaml
name: test

# Unit-test matrix on real OS runners. This is where Windows tests
# actually EXECUTE — the local Docker matrix (scripts/test-matrix.sh)
# can only compile-check the Windows target from a macOS host.

"on":
  pull_request:
  push:
    branches: [master, develop]

jobs:
  unit:
    name: unit (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-24.04, windows-latest, macos-latest]
    steps:
      - name: Checkout
        uses: actions/checkout@v4

      - name: Install system dependencies (Linux)
        if: runner.os == 'Linux'
        run: |
          sudo apt-get update
          sudo apt-get install -y \
            libwebkit2gtk-4.1-dev \
            libgtk-3-dev \
            libayatana-appindicator3-dev \
            librsvg2-dev \
            libssl-dev \
            patchelf

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable

      - name: Cache cargo registry and target
        uses: Swatinem/rust-cache@v2
        with:
          workspaces: src-tauri
          key: test-${{ matrix.os }}

      - name: Setup Node.js
        uses: actions/setup-node@v4
        with:
          node-version: '20'

      - name: Setup pnpm
        uses: pnpm/action-setup@v4
        with:
          version: 10.32.1
          run_install: false

      - name: Install npm dependencies
        run: pnpm install --frozen-lockfile

      - name: Ensure frontendDist exists for tauri-build
        shell: bash
        run: mkdir -p dist

      - name: Rust unit tests
        run: cargo test --manifest-path src-tauri/Cargo.toml --lib

      - name: TypeScript unit tests
        run: npx tsx src/components/ChatView/parser.test.ts
```

- [ ] **Step 2: Validate workflow syntax locally**

Run: `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/test.yml')); print('yaml ok')"`
Expected: `yaml ok`. (If PyYAML is missing: `pip3 install pyyaml` or `npx -y yaml-lint .github/workflows/test.yml`.)

- [ ] **Step 3: Checkpoint**

The workflow can only be end-to-end verified after push, which requires an explicit user commit/push request. Note this explicitly in the final report: "test.yml is written but unverified until pushed; first run on windows-latest may surface runner-specific issues (watch the Actions tab on the first PR)."

---

### Task 7: Document the matrix

**Files:**
- Modify: `docs/TESTING.md` (insert new section before `## Logging`, bump revision at bottom)

- [ ] **Step 1: Add the section**

Insert into `docs/TESTING.md` before the `## Logging` heading:

````markdown
## Cross-platform test matrix

One command runs unit tests for all three OS targets in parallel from a macOS host:

```bash
./scripts/test-matrix.sh                 # all lanes
./scripts/test-matrix.sh linux           # single lane
```

| Lane | Where | What runs | Coverage |
|------|-------|-----------|----------|
| `macos` | native host | `cargo test --lib` + TS parser tests | full execution |
| `linux` | Docker (ubuntu:24.04, aarch64) | `cargo test --lib` + TS parser tests | full execution, isolated |
| `windows-cross` | Docker + cargo-xwin | `cargo check --target x86_64-pc-windows-msvc --lib --tests` | compile-level only |

Windows containers cannot run on a macOS Docker host, so full Windows test
**execution** happens in CI (`.github/workflows/test.yml`, 3-OS matrix on every
PR) and via remote E2E against a real Windows machine. The local `windows-cross` lane still
catches `#[cfg(windows)]` compile breakage in minutes without leaving this machine.

Logs land in `$TMPDIR/codeterm-test-matrix/<lane>.log`. Containers shadow
`src-tauri/target` and `node_modules` with named Docker volumes
(`codeterm-linux-*`, `codeterm-xwin-*`) — the host checkout is never dirtied,
and warm reruns are incremental. To reclaim disk: `docker volume rm $(docker volume ls -q | grep '^codeterm-')`.
````

- [ ] **Step 2: Bump revision**

In `docs/TESTING.md`, change `<!-- revision: 4 -->` to `<!-- revision: 5 -->` (or current+1 if it drifted).

- [ ] **Step 3: Checkpoint**

Run: `git diff --stat docs/TESTING.md`
Expected: one file changed, insertions only plus the revision line. Do not commit.

---

## Final verification (whole plan)

- [ ] `./scripts/test-matrix.sh` → all three lanes PASS, parallel, exit 0.
- [ ] `git status --porcelain` → only the files in the file map above.
- [ ] Report to user: lane timings, any Windows-target findings from Task 3, and the note that `test.yml` needs a push to verify. Ask whether to commit.
