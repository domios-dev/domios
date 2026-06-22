# Prior Art: Generic Interactive-Prompt Detection & Interception

Research note for the agent-state-spec. Goal: CodeTerm should intercept ANY
interactive terminal prompt (pickers, menus, yes/no, permission prompts)
**generically** — without per-provider menu fingerprints — and surface it in the
chat UI. CodeTerm owns the PTY master (`portable-pty`) per agent pane and already
has a provider-agnostic `SignalExtractor` (vt100 grid diff + two-clock
quiescence, `src-tauri/src/agent/signal_extractor.rs`) and a
`SemanticMarkerParser` (OSC 133/633/7/9/1337, `src-tauri/src/agent/semantic_marker.rs`).

---

## TL;DR recommendation

1. **Termios/console-mode-from-the-master is NOT feasible cross-platform.** Drop
   it. The slave's line discipline (`~ICANON`/`~ECHO`/raw) is invisible to the
   master on both Unix and Windows ConPTY. Details + sources in §1.
2. **The practical generic detector is a small state machine over the OUTPUT
   byte stream**, keying on DEC private modes already parseable beside the
   `SignalExtractor`:
   - **`?1049h`/`?1049l` (+ legacy `?47`/`?1047`) = full-screen TUI open/close.**
     High precision, zero false positives. But misses inline prompts.
   - **`?25l`/`?25h` (hide/show cursor, DECTCEM) = the least-common-denominator
     bracket for INLINE pickers** (inquirer.js, prompts, enquirer, dialoguer,
     inquire, bubbletea, questionary, survey, fzf, gh all hide the cursor while a
     selection is active). This is the signal that catches the permission/menu
     prompts the alt-screen test misses.
   - **`?2004h` (bracketed paste) is an ANTI-signal** — bash/zsh readline turn it
     on by default for every line prompt. Use it to *exclude* "normal shell
     waiting for a line", never to assert a picker.
   - Mouse tracking (`?1000/1002/1006h`) = good corroboration; DECCKM `?1h`,
     keypad `ESC =` = too provider-dependent to rely on.
3. **Combine all three:** alt-screen OR (cursor-hidden + quiescent + no
   esc-to-interrupt), with bracketed-paste-only treated as "shell line prompt,
   not a picker." Corroborate with the existing quiescence clock (cursor parked,
   screen still) to suppress spinner/progress-bar flicker that also hides the
   cursor.
4. **Interception model = iTerm2-Triggers / expect:** regex on the raw stream →
   categorize → action. For the three first-class agents (Claude/Codex/Cursor),
   prefer their **structured permission channels** over scraping the TUI (§2).
5. **Auto-respond only an explicit allowlist; default to asking the human.**
   Hard-deny host-key/TOFU, `[y/N]`-default, and danger-vocabulary prompts.
   Circuit-break repeated identical prompts to avoid auto-answer loops (§3).

---

## 1. The general terminal mechanism — ranked

### 1a. Termios / console-mode from the master — NOT FEASIBLE (rejected)

**Unix.** A pty pair has **two independent `termios` structs**, one per half;
they are not shared or synced. `tcgetattr()` on the master fd returns the
*master's own* (vestigial) termios, never the slave's — so it tells you nothing
about whether the child has cleared `ICANON`/`ECHO` to read menu keypresses. The
Linux kernel even carries a patch titled "the pty master's termios should never
be set". There is no portable API for the master to read the slave's `c_lflag`.

The one notification path, packet mode `TIOCPKT` / `TIOCPKT_IOCTL`, fires **only
when the slave sets the `EXTPROC` (LINEMODE) bit** — a telnet-era feature a
normal CLI entering cbreak/raw does **not** set. On Linux it doesn't even ship
the new termios; it only toggles the flow-control `DOSTOP`/`NOSTOP` bits. So
packet mode is a flow-control/flush/LINEMODE channel, not an ICANON/ECHO
observability channel.

`portable-pty`'s `MasterPty::get_termios()` reads the master's own struct (no
help); the `nix` crate has no `TIOCPKT` wrapper.

- POSIX tcgetattr: https://pubs.opengroup.org/onlinepubs/9699919799/functions/tcgetattr.html
- Linux ioctl_tty(2): https://www.man7.org/linux/man-pages/man2/ioctl_tty.2.html
- TIOCPKT(2const): https://www.man7.org/linux/man-pages/man2/TIOCPKT.2const.html
- pty(7): https://www.man7.org/linux/man-pages/man7/pty.7.html
- kernel pty.c: https://github.com/torvalds/linux/blob/master/drivers/tty/pty.c
- "master termios should never be set": https://lkml.iu.edu/hypermail/linux/kernel/1410.2/00596.html
- macOS/BSD pty(4) `TIOCPKT_IOCTL`+`TIOCEXT`: https://www.daemon-systems.org/man/pty.4.html
- portable-pty MasterPty: https://docs.rs/portable-pty/latest/portable_pty/trait.MasterPty.html
- nix termios: https://docs.rs/nix/latest/nix/sys/termios/index.html

**Windows ConPTY.** No pseudoconsole API exposes the child's input mode
(`ENABLE_LINE_INPUT`/`ENABLE_ECHO_INPUT`) to the owner of the pseudoconsole
handles. Confirmed by a Microsoft Terminal maintainer: "I don't think we provide
any sort of way to call the console APIs on a conpty you create."
`CreatePseudoConsole` covers only create/resize/close — no mode introspection.

- ConPTY maintainer answer: https://github.com/microsoft/terminal/issues/13201
- CreatePseudoConsole: https://learn.microsoft.com/en-us/windows/console/createpseudoconsole

**Verdict:** the slave's mode is not observable from the master on either
platform. The only genuinely cross-platform surface is the **bytes the child
writes** — i.e. DEC private-mode escape sequences. This is exactly why tmux and
other multiplexers track alt-screen / mouse state by parsing output, not termios.

### 1b. DECSET private modes in the output stream — the practical path

Reference: xterm ctlseqs (https://invisible-island.net/xterm/ctlseqs/ctlseqs.html),
ECMA-48 (https://www.ecma-international.org/publications-and-standards/standards/ecma-48/).

The signals split into two classes — full-screen apps vs lightweight inline
prompts — and the inline class is the hard, important one (permission prompts
draw inline, not on the alt screen).

| Signal | Sequence | Reliability as "picker open" | Notes |
|---|---|---|---|
| **Hide cursor (DECTCEM)** | `?25l` / `?25h` | **Best least-common-denominator** | Emitted by inquirer.js/prompts/enquirer (via sindresorhus `cli-cursor`), dialoguer (`console::hide_cursor`), inquire (crossterm `Hide`), bubbletea (default), questionary/prompt_toolkit, survey, fzf, gh. Catches INLINE pickers. Caveat: also fired by spinners/progress (ora, indicatif) — debounce + require quiescence. |
| **Alt screen** | `?1049h`/`l` (legacy `?47`,`?1047`) | **Most specific, ~zero FP** | vim/less/htop/nano/fzf-fullscreen/tig/lazygit. bubbletea only with `WithAltScreen()`; prompt_toolkit only `full_screen=True`. Inline pickers do NOT enter alt screen → necessary but not sufficient. |
| **Mouse tracking** | `?1000/1002/1003/1006h` | Good corroboration | fzf (incl. inline `--height`), vim/tmux/htop. Most keyboard-only inline pickers don't enable it. Secondary signal. |
| **App cursor keys (DECCKM)** | `?1h`/`l` | Weak | Inline pickers mostly parse both CSI+SS3 arrow forms and don't set it. Skip as primary. |
| **Keypad app mode** | `ESC =` / `ESC >` | Too rare/legacy | Not usable alone. |
| **Bracketed paste** | `?2004h`/`l` | **ANTI-signal** | bash≥4.4 / Readline≥7 / zsh≥5.1 enable it by default on every line prompt. Use to EXCLUDE the normal shell-line case. |

Sources for inline-picker cursor-hide behavior:
- cli-cursor (`[?25l`/`h`, used by inquirer/prompts/ora): https://github.com/sindresorhus/cli-cursor/blob/main/index.js
- bubbletea (default cursor hide, `WithAltScreen` opt-in): https://pkg.go.dev/github.com/charmbracelet/bubbletea
- prompt_toolkit alt-screen only when full_screen: https://python-prompt-toolkit.readthedocs.io/en/master/pages/advanced_topics/rendering_pipeline.html
- dialoguer Select / console hide_cursor: https://docs.rs/dialoguer/latest/dialoguer/struct.Select.html
- bracketed-paste default-on: https://cirw.in/blog/bracketed-paste , https://archive.zhimingwang.org/blog/2015-09-21-zsh-51-and-bracketed-paste.html

### 1c. Cursor + quiescence (already in SignalExtractor)

The existing two-clock quiescence (`byte_idle_ms`, `semantic_idle_ms`,
`cursor_idle_ms`, `Motion::Still`) and `CursorSignal` are exactly the
corroboration needed to turn a noisy `?25l` into a confident "awaiting keypress":
a picker is **quiescent** (screen still, cursor parked, no streaming) AND has the
cursor hidden / alt-screen active. A spinner that hides the cursor keeps churning
the volatile region (`Motion::Animating`), so requiring `Motion::Still` +
`?25l`-active discriminates a parked menu from an animation. The absence of an
"esc to interrupt" affordance (a vendor-agnostic *secondary* hint, kept out of
the core detector) further separates "awaiting input" from "working".

### Recommended general detector (provider-agnostic)

Add a DECSET-tracking layer beside `SemanticMarkerParser` (it already walks the
ESC/CSI stream; extend it to record private-mode set/reset), feeding a small
state machine fused with `StreamSignals.quiescence`/`cursor`:

```
in_interactive_prompt :=
    alt_screen_active                       // ?1049h seen, no matching l
 OR ( cursor_hidden                         // ?25l seen, no matching h
        AND quiescence.kind == Still        // screen settled (debounced)
        AND NOT bracketed_paste_only )      // exclude plain readline line-prompt
 [ corroborated_by mouse_tracking_active ]  // raises confidence, not required
```

This is robust because it does not match any vendor's menu text — it keys on the
terminal-protocol brackets the picker emits to take over the keyboard, plus the
mechanical quiescence the `SignalExtractor` already computes.

---

## 2. Prior art worth reusing (with URLs)

The field splits on *how it knows a prompt is waiting*: shell hooks (OSC 133,
shell-cooperative — useless against an opaque agent CLI), pure output detection
(regex/heuristics — the only thing that generalizes), structured channels (the
program emits a machine-readable request — best when available), and avoidance
(env/flags — requires cooperating with the target, not reusable by a watcher).

### Tier 1 — directly reusable, shell-independent

- **iTerm2 Triggers — the single most on-point template.** ICU regex matched on
  the raw PTY stream line-by-line → fires an action (`Send Text` to inject an
  answer, `Prompt Detected` to *synthesize* prompt boundaries with no OSC 133,
  `Post Notification`, `Open Password Manager`, `Capture Output`). The **`Instant`
  flag** fires mid-line without waiting for a newline — added specifically because
  "password prompts usually are not followed by a newline." This newline-less
  prompt is the key gotcha to design for. https://iterm2.com/documentation-triggers.html
- **iTerm2 Coprocesses** — bidirectional pipe: all terminal output → coprocess
  stdin, coprocess stdout → injected keystrokes. The cleanest "external agent
  watches output and answers" model. https://iterm2.com/documentation-coprocesses.html
- **expect / pexpect / empty / sexpect** — canonical detect-then-respond: spawn on
  a PTY, accumulate output, match each chunk against a pattern list, `send` a
  canned response, time out gracefully (pexpect `EOF`/`TIMEOUT` pseudo-patterns
  are the reference for "I never truly know a prompt is waiting"). **sexpect's
  daemon + thin-CLI-client split mirrors CodeTerm's daemon + multi-pane model
  best.** https://pexpect.readthedocs.io/en/stable/overview.html ,
  https://github.com/clarkwang/sexpect , https://www.nist.gov/services-resources/software/expect

### Tier 2 — structured permission channels (prefer for known agents; per-vendor)

For the first-class agents, scraping the TUI is strictly inferior — they expose
machine-readable approval requests. Use these when the pane runs a known agent;
fall back to the §1 output detector only for TUI-only tools (aider,
open-interpreter, bare Codex/Cursor TUI, fzf, gh, npm, ssh, etc.).

- **Claude Code / Agent SDK** — `canUseTool` callback returns
  `{behavior:"allow"|"deny", updatedInput}`; `--permission-prompt-tool` routes
  approvals to an MCP tool returning JSON; headless `-p --output-format
  stream-json`. No TUI to scrape.
  https://code.claude.com/docs/en/agent-sdk/permissions
- **OpenAI Codex CLI** — `codex app-server` (JSON-RPC over stdio) emits
  server-initiated `item/commandExecution/requestApproval` +
  `item/fileChange/requestApproval`; client replies accept/decline.
  https://developers.openai.com/codex/app-server
- **cursor-agent** — Agent Client Protocol (ACP) `session/request_permission`
  JSON-RPC with `optionId` (allow-once/allow-always/reject-once), explicitly the
  way to manage permissions "without relying on terminal UI prompts." **ACP is
  the emerging cross-vendor standard** worth tracking. https://cursor.com/docs/cli/acp

### Tier 3 — not reusable by a generic watcher

- OSC 133/633/1337 shell hooks (Warp, WezTerm semantic zones, Kitty, VS Code
  region split + Quick-Fix triggering) — break without shell cooperation; useless
  vs an arbitrary agent CLI or raw TUI. Warp: https://docs.warp.dev/terminal/integrations-and-plugins/
  ; WezTerm: https://wezterm.org/shell-integration.html ; Kitty:
  https://sw.kovidgoyal.net/kitty/shell-integration/ ; VS Code shell integration +
  Quick Fixes: https://code.visualstudio.com/docs/terminal/shell-integration ,
  https://github.com/microsoft/vscode/pull/166326
- Avoidance flags/env (`yes(1)`, `DEBIAN_FRONTEND=noninteractive`/debconf,
  `apt-get -y`, ssh `StrictHostKeyChecking=accept-new`, `npm -y`) — cause the
  program to never ask; require modifying the target, so a watcher can't rely on
  them. https://www.gnu.org/software/coreutils/manual/html_node/yes-invocation.html
  , https://manpages.debian.org/trixie/debconf-doc/debconf.7.en.html

CodeTerm-relevant note: VS Code's `PartialCommandDetectionCapability` (on Enter,
if `cursorX >= 2`, assume a command ran) shows the floor of heuristic detection
without shell integration — coarse, no text/exit/region. CodeTerm's
`SignalExtractor` is already well past this.

---

## 3. Categorize → auto-respond, with safety guards

### Parsing the prompt's safe default

- The capitalized letter in `[Y/n]`/`[y/N]` is the default applied on bare Enter
  — widespread practitioner convention, NOT in GNU/POSIX normative text.
  https://linuxconfig.org/bash-script-yes-no-prompt-example
- Locale-aware affirmative parsing is normative: glibc `rpmatch()` honors the
  locale's `YESEXPR`/`NOEXPR`, but **`^[Yy]`/`^[Nn]` are always accepted
  regardless of locale** — use that as the floor.
  https://www.man7.org/linux/man-pages/man3/rpmatch.3.html
- POSIX `rm`: "If the response is not affirmative, rm does nothing more" — any
  non-yes is a "no". https://pubs.opengroup.org/onlinepubs/9699919799/utilities/rm.html
- **Implication:** a prompt with NO capitalized letter (plain `(y/n)`, ssh's
  `(yes/no)`) has no safe default — the tool is deliberately forcing an explicit
  choice. Never auto-answer those.

### Categories

| Category | Example | Auto-policy |
|---|---|---|
| Safe update/install (unprivileged), default = Yes | "Update available, install? [Y/n]" | Allowlist auto-yes |
| Rate-limit / retry | "retry in N seconds?" | Defer with backoff + jitter, capped; never blind-retry |
| Default-No (`[y/N]`) | "Overwrite? [y/N]" | Respect the safe default the tool already chose — escalate, don't auto |
| Host-key / TOFU trust | ssh `(yes/no)` fingerprint | **HARD DENY — never blind-accept** |
| Destructive | "delete N files?", "force push?", "drop database?" | **HARD DENY — escalate to human** |
| Typed-name confirm | "type the repo name to confirm" | Never auto-fill (anti-automation by design) |
| Unknown | anything unmatched | Default to ASKING the human |

### Safety guards (synthesized from real systems)

1. **Default-deny / allowlist-first.** Auto-answer only an explicit allowlist;
   everything else escalates to a human. Mirrors Claude Code's allowlist model and
   OWASP/NIST fail-closed design.
   https://code.claude.com/docs/en/agent-sdk/permissions ,
   https://cheatsheetseries.owasp.org/cheatsheets/Input_Validation_Cheat_Sheet.html
   , https://csrc.nist.gov/glossary/term/least_privilege
2. **Host-key/TOFU is never auto-accepted.** RFC 4251 §4.1: implementations
   "SHOULD NOT normally allow such connections by default"; blind-accept reopens
   MITM (GitHub's 2023 RSA host-key rotation is the cautionary case). Use
   pre-provisioned `known_hosts` / SSHFP / SSH-CA instead of answering.
   https://www.ietf.org/rfc/rfc4251.txt ,
   https://github.blog/news-insights/company-news/we-updated-our-rsa-ssh-host-key/
3. **Classify the triggering command, not just the prompt text.** Reuse prior-art
   danger regexes, whitelist-first (allow `rm -rf /tmp/*`, deny `rm -rf /`,
   `dd of=/dev/`, `mkfs`, fork bomb, `chmod -R 777`, `git push --force`, `git
   reset --hard`, `DROP DATABASE`). Command identity must be stable (the Ansible
   `become` lesson). destructive_command_guard:
   https://github.com/Dicklesworthstone/destructive_command_guard ; safe-rm:
   https://manpages.debian.org/bookworm/safe-rm/safe-rm.1.en.html ; thefuck ships
   dangerous fixes disabled-by-default: https://github.com/nvbn/thefuck
4. **Danger vocabulary = auto-deny.** "permanently", "cannot be undone",
   "irreversible", "force", "delete", "drop", "overwrite", "--no-preserve-root".
5. **Circuit-break to prevent auto-answer loops.** Per-pane: if the same prompt
   re-appears N times within a window, stop auto-answering and escalate. Add
   jitter to any timed defer/retry. https://martinfowler.com/bliki/CircuitBreaker.html
   , https://aws.amazon.com/builders-library/timeouts-retries-and-backoff-with-jitter/
6. **Audit every auto-response.** Log {pane, timestamp, prompt text, parsed
   default, answer sent, matched rule, triggering command} — the attributable
   record (NIST 800-53 AU-3). https://csrc.nist.gov/pubs/sp/800/53/r5/upd1/final
7. **Prefer dry-run / structured channel over scraping** where the tool supports
   it; never substitute auto-approve for human review on destructive/privileged
   ops. Terraform accepts only literal "yes":
   https://developer.hashicorp.com/terraform/tutorials/cli/apply

---

## Net architecture fit for CodeTerm

- **Detection:** extend the OSC scanner (`semantic_marker.rs` already walks
  `ESC`/`]`; add a CSI private-mode tracker) into a `DecPrivateMode` tracker that
  records the open/close state of `?1049`, `?25`, `?2004`, `?1000/1002/1006`, then
  fuse with `StreamSignals.quiescence`/`cursor` per the §1 state machine. Stays in
  the "core extracts generic signals, consumers interpret" layer — no vendor
  strings, satisfying the project's "structural fingerprints, never literal
  strings" rule.
- **Interpretation/extraction:** an iTerm2-Trigger / expect-style regex pass on
  the tail (for the answer set `[Y/n]` and prompt question) runs only once the
  generic state machine says "interactive prompt mode" — keeping vendor regex out
  of the hot per-byte path and bounded to the moment a prompt is actually up.
- **Response:** for known agents use the structured channel (Claude `canUseTool`,
  Codex app-server, Cursor ACP); for opaque TUIs, inject the answer via the
  master writer (the Coprocess model) under the §3 allowlist + guards.

<!-- revision: 1 -->
