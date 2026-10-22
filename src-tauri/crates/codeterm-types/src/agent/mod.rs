pub mod background_work;
pub mod chat_types;
pub mod context;
pub mod keys;
pub mod models;
pub mod provider_signals;
pub mod roles;
pub mod session_model;
pub mod tab_todo;
pub use roles::{
    is_legacy_cartographer_worker_binding, restore_role, AgentRole, ReportOutcomeClass,
    ReportStatus, RestoredRole,
};
pub use session_model::SessionModelTranscriptBoundary;
pub use tab_todo::{
    can_review_tab_todo, plan_tab_todo_write, tab_todo_bound_refusal, validate_tab_todo_reason,
    TabTodoAttemptSpan, TabTodoChecklist, TabTodoChecklistStatus, TabTodoCounters, TabTodoDraft,
    TabTodoEventKind, TabTodoItem, TabTodoItemEvent, TabTodoItemRef, TabTodoLink, TabTodoLinkDraft,
    TabTodoNewItem, TabTodoProposal, TabTodoProposalState, TabTodoRecitation, TabTodoRecitedItem,
    TabTodoState, TabTodoSummary, TabTodoWriteBase, TabTodoWriteEntry, TabTodoWritePlan,
    TabTodoWriteStateChange, TabTodoWriteTarget, MAX_TAB_TODO_DESCRIPTION_BYTES,
    MAX_TAB_TODO_ITEMS, MAX_TAB_TODO_ITEM_EVENTS, MAX_TAB_TODO_REASON_BYTES,
    MAX_TAB_TODO_SUMMARY_BYTES, MAX_TAB_TODO_SUMMARY_TITLES, MAX_TAB_TODO_TITLE_CHARS,
    MAX_TAB_TODO_URL_BYTES,
};
pub mod role_identity;
pub mod role_kind;
pub use role_kind::{RoleKindCapability, RoleKindDescriptor, RoleKindOrigin};

pub mod capability_probe;
pub mod launch_spec;
pub use launch_spec::LaunchSpec;
