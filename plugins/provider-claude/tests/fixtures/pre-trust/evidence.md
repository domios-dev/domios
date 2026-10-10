Project keys and record location

macOS: the captured Claude Code 2.1.296 folder-trust screen in
../folder-trust/claude-2.1.296-auto-mode.txt displays
/private/tmp/trustprobe.d1AhFE. The native and JS key builders resolve the exact
launch directory before selecting its project entry. Temp-directory symlink
coverage proves alias and resolved cwd select the same key. This is screen and
path-resolution evidence; the live trust-record persistence proof remains QA.

Windows: Claude Code's own diagnostic reproduced in Anthropic's upstream issue
https://github.com/anthropics/claude-code/issues/72610 (2.1.197, Windows) names
projects["C:/Users/<user>/.../update_things"].hasTrustDialogAccepted. The tests
use a sanitized C:/Users/alex/src/demo key: uppercase drive and forward slashes,
without folding the remainder's case. UNC paths keep their leading //.
A current Windows live entry and share-delete refusal are deferred to QA.

Linux: absolute realpath with POSIX separators is retained. The provider's
upstream report https://github.com/anthropics/claude-code/issues/27978 documents
$CLAUDE_CONFIG_DIR/.claude.json holding project/session data on Ubuntu 24.04.
Without that environment override the record is $HOME/.claude.json.
Relative configuration roots are skipped so the daemon cannot edit a record
resolved relative to a different cwd than the provider process.
Temp fixtures exercise these locations without reading any live account record.

Tests use temporary account roots exclusively. No live ~/.claude.json was read
or written to gather this evidence. Platform spelling beyond the cited versions
remains a hypothesis until the Orchestrator's live proof.
