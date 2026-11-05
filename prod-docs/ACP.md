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
| `gemini-cli` | Install Gemini CLI separately so Domios can resolve `gemini`. The profile launches `gemini --acp`. Authenticate using the agent's declared methods below. There is no built-in installer for this profile. |
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
domios agent spawn acp --profile local --model MODEL_ID --description "Work with the local agent"
```

`--profile` selects the launch command; `--model` selects a model within that
agent's session. Omit the model to use the agent's default. ACP reads the session's
declared model configuration; a requested model is applied only when advertised.
If no model option exists or selection fails, the tab shows a notice that the
requested model was not applied. Check the reported session model before claiming
that selection succeeded. Unknown, missing required, or uninstalled profiles
refuse before tab creation. Local profile setup is required even if its executable
already exists on PATH.

## Permission questions and cancellation

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

## Platform notes and limits

- The executable must be resolvable on the Domios host, including on Windows and
  macOS; an executable available only in an interactive shell may not be visible
  to the app. Prefer the installed executable's absolute path for local setup.
  There is no PTY in which to answer installation or workspace-trust prompts.
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

<!-- revision: 2 -->
