//! Relayed `tab list` rendering. The peer answers `GET /api/tabs` with a bare
//! array; the local CLI decides the shape so `--mesh` prints what local prints.

#[cfg(test)]
use crate::cmd::print::tab_list_json_envelope;
use crate::cmd::print::{print_json, tab_list_json_envelope_with_request};
use crate::{Cli, Commands, TabAction};

/// Which local formatter a relayed tab list is handed to. The peer's bare array
/// is never printed through, so no transport-dependent JSON shape can escape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TabListRendering {
    Tree,
    JsonEnvelope,
}

/// `None` means the command is not a tab list and stays on the generic relay.
pub(super) fn rendering_for(cli: &Cli) -> Option<TabListRendering> {
    if !matches!(
        cli.command,
        Commands::Tab {
            action: TabAction::List { .. }
        }
    ) {
        return None;
    }
    Some(if cli.json {
        TabListRendering::JsonEnvelope
    } else {
        TabListRendering::Tree
    })
}

/// Wrap a relayed bare `/api/tabs` array in the same envelope the local path
/// builds. Takes the body by reference: the REST response is not rewritten.
#[cfg(test)]
pub(super) fn json_envelope(relayed: &serde_json::Value) -> serde_json::Value {
    tab_list_json_envelope(relayed.as_array().cloned().unwrap_or_default())
}

pub(super) fn print(
    rendering: TabListRendering,
    relayed: &serde_json::Value,
    requested_workspace: Option<&str>,
) {
    match rendering {
        TabListRendering::JsonEnvelope => {
            print_json(&tab_list_json_envelope_with_request(
                relayed.as_array().cloned().unwrap_or_default(),
                requested_workspace,
            ));
        }
        TabListRendering::Tree => {
            let tabs = relayed.as_array().cloned().unwrap_or_default();
            print!(
                "{}",
                crate::cmd::tab::render_tab_tree_with_scope(&tabs, None, requested_workspace)
            );
        }
    }
}
