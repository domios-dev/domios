# Prompt catalog

The authoritative [prompt catalog](../../../src-tauri/prompts/00-README.md) lives beside the prompt sources. It lists every prompt, its audience, loader, placeholders and injection point. Update that catalog when adding or moving a prompt.

`mcp_tools.json` holds MCP tool and argument descriptions. Its documented loader
exception is the direct `include_str!` in `cli/src/cmd/mcp/tools.rs`: the CLI
cannot depend on `codeterm-prompts` without expanding its dependency closure.
