# Bitwarden

Use a [Bitwarden](https://bitwarden.com) vault as CodeTerm's secret store, so `codeterm mem secret` reads and writes land in your vault instead of the built-in local file store.

## What it does

- Becomes the active **secret backend** when you toggle "Use as your default secret store" in its detail pane.
- Unlock, status, and server URL live in the plugin's own view (a sandboxed iframe) — your master password is entered there, never in CodeTerm's main window.
- Optional default organization / collection: new secrets are filed there automatically.

## Requirements

- The Bitwarden CLI (`bw`) on your `PATH`. Install via `brew install bitwarden-cli`, `npm i -g @bitwarden/cli`, or the official download.
- A Bitwarden account (cloud `vault.bitwarden.com`, or a self-hosted server URL set in the view).

## Permissions

| Permission | Why |
|------------|-----|
| `subprocess: bw` | All vault operations go through the official `bw` CLI. |
| `network: vault.bitwarden.com` | Lets `bw` reach Bitwarden's cloud API. For self-hosted servers, the CLI is pointed at your host. |
| `secrets` | Lets the plugin serve as CodeTerm's secret backend. |

## Use

1. Install the plugin and open its detail pane.
2. Turn on **Use as your default secret store**.
3. In the connection view, set your server URL (if self-hosted) and unlock with your master password.
4. Optionally pick a default organization and collection.

Turning the toggle off falls back to the built-in local file store; your vault is untouched.
