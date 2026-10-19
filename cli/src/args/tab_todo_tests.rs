use clap::Parser;

use super::cli::{Cli, Commands};
use super::tab_todo::TabTodoAction;
use super::todo::TodoAction;

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
