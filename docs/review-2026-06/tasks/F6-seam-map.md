# F6 seam map — cli/src/cmd/plugin.rs (1743 lines)

Produced by read-only probe (big-pickle), 2026-06-11. Executor: verify line ranges
before relying on them — the file may have drifted.

## Proposed layout: `cmd/plugin/`

| File | ~Lines | Contents |
|---|---|---|
| mod.rs (facade <600) | small | `handle_plugin` dispatch (orig 159-188), `print_human`, re-exports |
| types.rs | ~200 | structs/enums/constants (orig 1-157), `is_reserved_system_id` |
| install.rs | ~250 | install_command, install_from_dir, copy_dir_filtered, should_skip, remove_installed, manifest readers (orig 223-300, 914-1000) |
| update.rs | ~250 | update_command, update_one, update_system_command, removed_system_channel_warnings, ensure_system_channel, compat_* (orig 302-466, 842-860) |
| channel.rs | ~300 | channel dispatcher, diff/materialize/classify_drift, add/remove/list/virtual, refresh_*, find_channel_plugin, plugin_path, clone_git_channel, parse_channel_source + path-validation helpers (orig 486-497, 586-912) |
| list.rs | ~200 | list_installed, merge_system_rows, system_rank, virtual_system_row (orig 499-584) |
| state.rs | ~150 | read_install_record, installed_ids, read/write_channel_registry, ALL path helpers incl `plugin_data_root` (orig 1044-1108) |
| audit.rs | ~200 | audit_*, command_failure_fields, append_audit_*, now_rfc3339 (orig 1110-1257) |
| version.rs | ~80 | last_change_*, version_cmp, semverish (orig 1002-1042) |
| util.rs | ~150 | looks_like_local_path, absolutize, system_channel_url, app_version, run_git[_in] (orig 1259-1440) |
| tests.rs | ~303 | `#[cfg(test)]` body of store_index_tests (orig 1441-1743); keep `mod tests;` in mod.rs |

## Risk notes (probe-verified)
- `update_one()` is the most entangled (touches install/state/version/audit/channel)
  but all deps are one-directional toward state/util/audit — NO cycles.
- `plugin_data_root()` is the global root for all I/O — lives in state.rs, import
  everywhere.
- channel registry file I/O is already lock-free/racy today — do not fix in this
  task, verbatim move only.
