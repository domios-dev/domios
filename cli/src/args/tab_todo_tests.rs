use clap::Parser;

use super::cli::{Cli, Commands};
use super::tab_todo::TabTodoAction;
use super::todo::TodoAction;

#[test]
fn checklist_proposal_commands_expose_review_flags() {
    assert!(matches!(
        Cli::try_parse_from(["codeterm", "todo", "submit"])
            .unwrap()
            .command,
        Commands::Todo {
            action: TodoAction::Submit
        }
    ));
    assert!(
        matches!(Cli::try_parse_from(["codeterm", "todo", "approve", "--tab", "worker", "--note-from-pad", "note.md"]).unwrap().command,
        Commands::Todo { action: TodoAction::Approve { tab, note_from_pad: Some(note) } } if tab == "worker" && note == "note.md")
    );
    assert!(
        matches!(Cli::try_parse_from(["codeterm", "todo", "amend", "--tab", "worker", "--from-pad", "todos.json"]).unwrap().command,
        Commands::Todo { action: TodoAction::Amend { tab, from_pad, .. } } if tab == "worker" && from_pad == "todos.json")
    );
    assert!(Cli::try_parse_from(["codeterm", "todo", "approve"]).is_err());
    assert!(Cli::try_parse_from(["codeterm", "todo", "amend", "--tab", "worker"]).is_err());
}

#[test]
fn agent_todo_write_uses_the_provider_native_json_pad_contract() {
    let cli =
        Cli::try_parse_from(["codeterm", "todo", "write", "--from-pad", "todos.json"]).unwrap();

    assert!(matches!(
        cli.command,
        Commands::Todo {
            action: TodoAction::Write { from_pad }
        } if from_pad == "todos.json"
    ));
}

#[test]
fn tab_todo_reopen_names_the_item_child_and_reason() {
    let cli = Cli::try_parse_from([
        "codeterm",
        "tab",
        "todo",
        "reopen",
        "item-1",
        "--tab",
        "team-child",
        "--reason",
        "QA rejected the check",
    ])
    .unwrap();
    assert!(matches!(
        cli.command,
        Commands::Tab {
            action: super::misc::TabAction::Todo {
                action: TabTodoAction::Reopen { item_id, tab, reason }
            }
        } if item_id == "item-1" && tab == "team-child" && reason == "QA rejected the check"
    ));
}

#[test]
fn tab_todo_commands_expose_parent_inspection_and_qa_generation_fence() {
    let inspect =
        Cli::try_parse_from(["codeterm", "tab", "todo", "list", "--tab", "team-child"]).unwrap();
    assert!(matches!(
        inspect.command,
        Commands::Tab {
            action: super::misc::TabAction::Todo {
                action: TabTodoAction::List {
                    tab: Some(tab)
                }
            }
        } if tab == "team-child"
    ));

    let append = Cli::try_parse_from([
        "codeterm",
        "tab",
        "todo",
        "append",
        "--tab",
        "team-child",
        "--generation",
        "4",
        "--from-pad",
        "checks.json",
    ])
    .unwrap();
    assert!(matches!(
        append.command,
        Commands::Tab {
            action: super::misc::TabAction::Todo {
                action: TabTodoAction::Append { generation: 4, .. }
            }
        }
    ));
}
