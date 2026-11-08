# ACP agents

Domios's `provider-acp` plugin (provider id `acp`) connects Agent Client Protocol
v1 agents over stdio JSON-RPC. An ACP tab is a `chat_backend` runtime with no PTY
or interactive shell. Send chat to it; do not send shell setup commands to it.
The agent can request host filesystem and terminal tools during a turn.

## Choose a launch profile

Discover the installed provider with `domios agent providers`. ACP requires an
explicit profile; a model id does not select an agent executable.

| Profile | Requirements and setup |
|---|---|
| `gemini-cli` | Install Gemini CLI separately so Domios can resolve `gemini`. The profile starts `gemini --acp` from a Domios-owned folder while its sessions open in the tab's folder, so the chat cleanup Gemini runs on each start never deletes another Gemini tab's saved session; your own `~/.gemini` settings are not changed. Gemini labels its shell rows `[current working directory …]` with that Domios-owned launch folder, because the label comes from Gemini's own process; the commands still run in the tab's project folder. A resumed session reads your user settings but not the project's `.gemini/settings.json`. A `gemini` you start yourself in that project can still prune Domios sessions it cannot resume, such as one that has no messages yet. Authenticate using the agent's declared methods below. There is no built-in installer for this profile. |
| `deepseek-harness` | In the Spawn picker, select ACP and this profile, choose **Install profile**, confirm the exact package, then **Confirm setup**. The pinned package is `@deepseek-ai/dsh@0.2.1-rc.5`, with `--profile acp`. Installation requires Node.js, npm and access to the npm registry. The installed runtime uses Node and the installed script, rather than fetching a package on every launch. |
| `local` | An owner-confirmed installed ACP executable and argument array. Configure it through the Spawn picker as described below. The placeholder `local-acp-command` is not an installed agent. |

DeepSeek requires the secret `domios:provider:acp:deepseek-api-key` before runtime
launch. Store the owner's key through Settings → Secrets, or feed it on stdin to:

```bash
domios mem secret save --name domios:provider:acp:deepseek-api-key
```

Use `--overwrite` only when replacing an existing entry. The profile injects this
secret as `DEEPSEEK_API_KEY` through `secretEnv`; a missing key refuses launch.
This profile does not declare an `authEnv` key-taking login method. Never place a
key in arguments, a task body or memory. See `domios docs SECRETS`.

## Connect another ACP agent

The `local` profile is the entry point for another installed ACP v1 agent:

1. Install the agent on the machine running Domios, using its own installation
   instructions. It must expose an ACP stdio endpoint.
2. Open the Spawn picker, select ACP, then **Agent profile** → `local`.
3. Choose **Configure local command**. Enter **Agent executable** and
   **Arguments (JSON array)**, such as `["--stdio"]` only if that agent requires
   that argument. Arguments are separate strings, not a shell command line.
4. Check **I trust this executable and these arguments**, then **Confirm setup**.
   Domios resolves the executable and saves an owner-confirmed launch receipt.
5. Spawn with `--profile local` or use the picker after installation reports ready.

### Manage profiles through the CLI

The local owner and live Team Orchestrators can manage profiles on this machine.
These commands refuse `--mesh`. Human-readable output is the default; add `--json`
to read the shared `result` envelope.

```bash
domios agent acp list
domios agent acp add myagent --binary /absolute/path/to/agent --arg --stdio --env MODE=production
domios agent acp add keyedagent --binary installed-agent --secret-env TOKEN=domios:provider:acp:myagent-token --auth-env API_KEY=domios:provider:acp:myagent-key:api-key
domios agent acp remove myagent
```

To add a variant of an existing profile, name it with `--like`. The new profile
copies that profile's declared icon, registry icon reference, client capabilities,
limits source and Domios-owned process folder; it never copies the launch command, environment or credential
mappings, so supply those yourself. Override a copied client capability with
`--client-capability NAME=true|false`, where `NAME` is `fs.readTextFile`,
`fs.writeTextFile` or `terminal`; undeclared names are refused.

```bash
domios agent acp add gemini-yolo --like gemini-cli --binary gemini --arg --acp --arg --yolo --env GEMINI_CLI_TRUST_WORKSPACE=true --auth-env GEMINI_API_KEY=domios:provider:acp:gemini-api-key:gemini-api-key
domios agent acp add gemini-lite-files --like gemini-cli --binary gemini --arg --acp --client-capability fs.writeTextFile=false
```

Both examples keep Gemini's icon, its `fs.readTextFile=false` workaround,
which lets Gemini CLI create new files through its own reader, and its
Domios-owned process folder.

`--limits-source` states where the usage bubble reads the profile's limits:

- `account:PROVIDER` shows that native provider's account limits exactly as its
  own tabs show them, gated by that provider's Usage Tracking setting; for
  example `account:claude` for a Claude ACP adapter signed in to the same account.
- `balance:READER:ENV` lets the plugin read an account balance with the profile's
  own credential `ENV`, which must be one of its `--secret-env` or `--auth-env`
  variables. The host places the key only in that request's header; it never
  appears in arguments, logs or the plugin. `deepseek-balance` is the bundled reader.
- `unavailable:REASON` shows a quiet dash, with the one-line reason (at most 240
  characters) in its popover. If the session reports a context window, the bubble
  shows that fill instead and keeps the reason in its popover.

Plugins can also declare a `quota` source, which reads account quota with the
credentials the plugin's manifest declares. `--limits-source` does not set it, but
`--like gemini-cli` copies it. Copied sources can be overridden the same way.

A profile installed from the ACP registry needs no flag: the plugin declares a
default per registry id, applied whenever profiles load, so already-installed
profiles pick it up without reinstalling. `claude-acp` and `codex-acp` show the
Claude and Codex account limits; `gemini` reads Gemini quota as described below.
A profile's own source always wins; `domios agent acp registry
install REGISTRY_ID --limits-source unavailable:REASON` overrides the default at
install. Other registry agents have no default.

The bundled `deepseek-harness` profile reads its DeepSeek balance. Profiles without
a source keep the runtime's own context and cost reading.

The bundled `gemini-cli` profile follows Gemini CLI's selected sign-in mode:

- Signed in with Google (`oauth-personal`), the bubble shows the percentage used on
  the model closest to its limit, with every model and its reset time in the popover.
  This reads Gemini Code Assist quota with the token Gemini CLI stores in
  `~/.gemini/oauth_creds.json`. The host places the token only in that request's
  header. Domios never renews it, so after the token expires the read waits until
  Gemini CLI renews it. The endpoint is internal to Gemini CLI and can change.
- With an API key or Vertex AI, no quota endpoint is read. The bubble shows the
  context fill Gemini reports for the session, or a dash until it reports one, and
  its popover explains why.

An account that requires `GOOGLE_CLOUD_PROJECT`, or a login kept in encrypted
storage, shows the same dash or context fill with its reason.

Names contain one word of letters, digits, underscores or hyphens; Windows device
names and existing profile names are refused. Repeat `--arg` for each argument,
including arguments beginning with a hyphen. `--env` carries non-secret values.
All environment options refuse `CODETERM*` names and duplicate variables.
`--secret-env` requires a named stored secret at launch. `--auth-env` maps an
optional secret to an agent-declared key-taking authentication method; its final
colon separates the method from the secret name. References must use the
`domios:provider:acp:` namespace. Store values through Settings → Secrets or the
stdin-only secret command before launching; never put secret values in arguments.

Adding a profile resolves its installed executable and saves the same kind of
owner-confirmed command receipt as the local Spawn form. Profiles persist outside
workspaces and appear in the Spawn picker. Select them with `agent spawn acp
--profile NAME`. Remove accepts owner-added profiles only and leaves installed
package files intact. Built-in profiles remain available.

The list reports installation readiness and whether referenced credentials are
configured or missing. Credential presence does not prove an authenticated agent
session; profiles without credential mappings report authentication as `unknown`.

In list JSON, `iconUrl` refers specifically to a registry-hosted icon URL. It is
`null` for bundled icons such as Gemini and DeepSeek. Bundled icon paths and their
sanitized artwork are exposed as `icon` and `iconSvg` on the provider's launch
profile descriptor; a profile with neither uses the generic provider icon.

Spawn setup returns an accepted job immediately. The picker follows setup events
and keeps the install status visible until installation succeeds or fails. Closing
the picker does not cancel an accepted installation; reopening it reads the latest
job and installation state. A cold package download can take longer than 30 seconds
without becoming a request-timeout failure. See `domios docs API-SPAWN` for the
accepted response and event contract.

Registry search and install share the same 20-second index-fetch deadline. A
refusal reports a bounded cause, such as HTTP status, request timeout, connection
failure or malformed JSON; an unavailable index is not a package-install failure.

Local commands must name an installed executable. Domios refuses `npx`, `npm`,
`uvx`, `uv` and `env` launchers, and shell-command indirection is not supported.
Use the installed agent executable, or an installed runtime with its installed
agent script as an argument, rather than a package-fetching launcher.

## Discover agents in the ACP registry

The plugin declares these agent verbs, exposed through the CLI plugin passthrough:

```bash
domios plugin acp registry-refresh
domios plugin acp registry-search gemini
domios plugin acp registry-refresh deepseek
```

Both verbs accept an optional query. `registry-search` searches the local cache by
id, name and description; before the first refresh, an absent cache returns an
empty list. `registry-refresh` fetches the ACP v1 registry, validates and saves it,
then searches it. A failed refresh retains the previous cache. These verbs require
the plugin's `agent_commands` grant and refresh requires network access.

### Install a registry profile

```bash
domios agent acp registry search gemini
domios agent acp registry install REGISTRY_ID
```

These commands fetch the bounded ACP v1 index directly. Installation creates an
owner profile with the registry id, declared arguments, non-secret environment and
icon reference. Review the publisher and distribution before installing. Existing
profile names are refused; remove an owner profile before replacing it.

- `npx`: install the registry's exact version through the same integrity-checked
  npm installer as DeepSeek Harness. Requires Node.js and npm. Runtime launches
  use the installed script without a package fetch.
- `uvx`: install the exact version into an isolated uv tool directory. Requires uv,
  an installed system Python and prebuilt wheels. Configuration discovery, source
  builds and Python downloads are disabled. Packages with multiple executables
  require manual installation and a local profile instead.
- Binary: select the current OS/architecture and require its declared SHA-256.
  Verify the download before reading its declared executable. Archive distributions
  require a compatible `tar`; only the executable is materialized. Distributions
  needing additional adjacent resources require manual installation. The installed
  executable checksum is rechecked before use.

Unpinned package tags/ranges, missing binary checksums, unsafe executable paths,
reserved environment names and unsupported distributions refuse installation.
Registry installation needs network access and does not establish authentication
or protocol compatibility. Configure credential references with a local profile
when an agent requires an environment mapping absent from its registry entry.

## Authenticate

After creating a tab, inspect its declared authentication methods:

```bash
domios agent login --tab TAB_ID --show
domios agent login --tab TAB_ID --method METHOD_ID
```

`--show` reads state without starting a flow. Use a method id returned by this
agent; unavailable or undeclared methods refuse. ACP `agent` methods are callable;
methods declared as `terminal` are unavailable through this non-PTY interface.

For a key-taking method, feed the key on stdin to the following command; never
include it in argv or send it over mesh:

```bash
domios agent login --tab TAB_ID --method METHOD_ID --api-key-stdin
```

Key-taking methods must be mapped by the selected profile's `authEnv`. Domios
stores the key in its secret store, restarts the runtime with the mapped variable,
then calls the declared authentication method. The Gemini profile maps method
`gemini-api-key` to secret `domios:provider:acp:gemini-api-key` and environment
variable `GEMINI_API_KEY`. Use it only if the running agent declares that method.
The local profile has no such mapping, so `--api-key-stdin` cannot provision an
arbitrary local agent's key.

OAuth/browser or device authentication is owned by the agent's declared method:
select that method with `--method ID` and complete the agent's supported flow.
Do not assume every ACP agent supplies a Domios link/code card. The generic
`agent login --device` flow requires a separate runtime login capability, which
this ACP plugin does not declare; it is not an alias for an ACP method id.
An agent requiring an interactive terminal login must be authenticated separately
using its own CLI before use here. Auth state can remain `unknown` until evidence
is available; an installed profile is not proof of authentication.

## Spawn and select a model

```bash
domios agent spawn acp --profile gemini-cli --description "Work with Gemini"
domios agent spawn acp --profile deepseek-harness --description "Work with DeepSeek"
domios agent models acp --profile local
domios agent spawn acp --profile local --model MODEL_ID --reasoning-effort EFFORT_ID --description "Work with the local agent"
```

`--profile` selects the launch command; `--model` selects a model within that
agent's session. `--reasoning-effort` selects the declared `thought_level` choice.
Omitting either flag preserves the agent's default for that option. The models
command shows this profile's last declared model and effort option IDs, values and
labels; an empty catalog means no choices have been observed yet. These cached
choices are advisory. Each new or restored session validates requested values
against its own declarations and confirms them before the bootstrap task is sent.
An unavailable value fails readiness with the offered values; a rejected or
unconfirmed change also fails readiness. The task is never silently sent with a
different selection. The same selection applies to plan dispatch/respawn and HTTP
spawn (`model` and `reasoning_effort` alongside the profile). Unknown, missing required, or uninstalled profiles
refuse before tab creation. Local profile setup is required even if its executable
already exists on PATH.

## Permission questions and cancellation

New ACP tabs run with no permission questions, as other Domios agents do. Change this
per tab in the model picker; the choice applies at once and survives restart and restore:

- **Agents with an approve-all mode** (Gemini YOLO, `claude-acp` bypassPermissions,
  `codex-acp` full-access): the agent's **Mode** row is the only switch. If you choose
  that mode, Domios also answers any question the agent still asks, such as Gemini's
  prompt about paths outside the project. If you choose any other mode, the agent asks.
  Domios re-applies your chosen mode after every restore.
- **Other agents** (DeepSeek, for example): a **Permissions** row offers **Ask each
  time** and **Allow all**. A question already waiting follows the new setting.

Only you can change these. An agent's request to change its mode or Permissions is
refused, and a mode the agent enters by itself doesn't change what Domios answers.

If the picker shows **Permissions: Full access (host)** beside a Mode row, the agent
isn't in its approve-all mode (it refused the mode or left it), but Domios is still
answering its questions.

`domios tab status --json` reports the effective state as `full_access`, and
`domios agent restart` prints the same value. Tabs confined by a sandbox or a neighbor
grant keep their confinement and show no Permissions row.

**Allow all** answers the agent's questions; it does not widen what the agent itself
enforces. DeepSeek keeps its workspace-write sandbox, and a denied command is retried
with approval, which Domios gives. So the command still runs, one retry later.

When the tab status is `question`, inspect the current prompt and numbered options:

```bash
domios tab status --tab TAB_ID
domios tab inspect TAB_ID
domios tab respond N --tab TAB_ID
```

Choose the displayed approve or reject option using its one-based number `N`;
numbers depend on the agent's declared choices. Do not assume option 1 approves.
Respond only within your authority. Domios binds the answer to the pending request;
raw keys are not the interface. Filesystem and terminal requests use host permission
gates too; granting a tool request does not bypass host access restrictions.

```bash
domios tab stop --tab TAB_ID
```

Stop interrupts the active turn via ACP `session/cancel`; it does not close the tab.
It can also cancel a turn waiting for a permission response.

### Messages during a turn

An ACP turn takes no second prompt, so reports and owner messages reach a busy agent
in one of three ways, in this order:

- An agent that advertises `_meta.steering.supported` at initialize, such as
  `claude-agent-acp`, takes them into the running turn right away.
- Any agent with Domios tools receives them in the next successful Domios tool result,
  under an `Inbound messages` block. The message counts as delivered once that result
  was returned. An agent that makes no Domios tool call receives nothing this way, and
  `messages_check` lets it collect them between long steps.
- Anything still held when the turn ends goes out as one digest prompt, in arrival
  order, with each message identified. A command, an attachment or a recording keeps
  its own prompt and is never overtaken.

Each message is delivered once. A claim the host cannot confirm stays
`submit_unconfirmed` and is never replayed. Check with `domios tab send-status <send-id>`.

### Full access

A full-access launch enters the session mode its profile declares, chosen from the
modes the agent advertises at session start. If the agent does not advertise that
mode, or refuses it, the tab runs inside the agent's own sandbox and says so:
`domios tab status` reports `failure.code` `full_access_unavailable` with the reason,
including the modes the agent offers. `domios tab inspect` shows it as
`agent_runtime_state.accessFailure`, the tab shows it as a notice, and the parent
receives a report.

A sandbox can stop a Worker's `domios agent ack` from reaching Domios. Domios sees that
failed command in the session, reports it to the parent with its output, and shows
`failure.code` `worker_ack_failed` in `domios tab status`.

The local owner can switch the session mode from the CLI:

```bash
domios agent model --tab TAB_ID --runtime-mode MODE
```

This makes the same selection as the model picker. `MODE` must be one the session
advertises. Requests carrying a tab credential are refused with `owner_required`, so
an agent cannot change its own or another tab's mode this way.

## Platform notes and limits

- The executable must be resolvable on the Domios host, including on Windows and
  macOS; an executable available only in an interactive shell may not be visible
  to the app. Prefer the installed executable's absolute path for local setup.
  There is no PTY in which to answer installation or workspace-trust prompts.
- With Volta-managed Node on macOS, plain `npm i -g` can place an agent CLI outside
  the daemon's PATH. Use `volta install` or ensure the npm global bin directory is
  on that PATH, then check `domios agent acp list` for `install=detected` on profiles
  that detect installed CLIs.
- The Gemini profile sets `GEMINI_CLI_TRUST_WORKSPACE=true` on every platform,
  including Windows and macOS. Selecting it launches Gemini with that trust
  setting. The local profile does not inherit the Gemini profile's environment.
- Only ACP v1 over stdio JSON-RPC is supported by this plugin. An ordinary CLI,
  HTTP-only agent or incompatible protocol version cannot use the local profile.
- Resume/load, models, modes and configuration depend on what the agent advertises.
  Domios refuses unadvertised controls; not every ACP agent supports restoration.
- Host Domios MCP tools can be supplied during session creation/restoration. A
  failed session handshake may retry once without them and show a tools-unavailable
  notice. See `domios docs AGENT-CONTROL` for the tool and authority contract.
- Plugin registry discovery is bounded and cached. CLI profile installation uses
  the current index and refuses unsupported distributions; neither proves agent
  authentication or compatibility.

<!-- revision: 13 -->
