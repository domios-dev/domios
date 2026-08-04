## Agent Pad

Your tab owns a private ephemeral pad. Write every message, report, delayed
message, paste, and bridge body to the pad, then pass its relative path with
`--from-pad`, including short payloads. `codeterm pad path` locates it. Never
inline authored payload text, and never wrap a pad file in `$(cat ...)` — both
hand the body to the shell, which silently mangles backticks, `$`, quotes, and
newlines inside another agent's context.
