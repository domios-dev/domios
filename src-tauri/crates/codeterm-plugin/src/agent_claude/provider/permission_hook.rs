use crate::provider::PermissionHookCapability;
use codeterm_types::pty::PromptOption;

pub(super) fn capability() -> PermissionHookCapability {
    PermissionHookCapability {
        protocol: "claude-permission-request".into(),
        options: vec![
            PromptOption {
                label: "Allow".into(),
                value: "allow".into(),
            },
            PromptOption {
                label: "Deny".into(),
                value: "deny".into(),
            },
        ],
    }
}
