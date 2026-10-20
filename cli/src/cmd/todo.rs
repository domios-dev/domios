use crate::args::todo::TodoAction;
use crate::client::RpcClient;

pub fn handle(client: &mut dyn RpcClient, json_output: bool, action: TodoAction) {
    let from_pad = match action {
        TodoAction::List => None,
        TodoAction::Write { from_pad } => Some(from_pad),
    };
    crate::cmd::tab_todo::handle_agent(client, json_output, from_pad);
}
