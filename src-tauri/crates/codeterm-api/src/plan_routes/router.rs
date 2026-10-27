use crate::routes::AppState;
use crate::{plan_routes, task_routes};
use axum::{
    routing::{delete, get, patch, post},
    Router,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/plans", post(plan_routes::create))
        .route(
            "/api/plans/{id}/project",
            post(plan_routes::project::set_project),
        )
        .route("/api/plans/resolve-refs", post(plan_routes::resolve_refs))
        .route(
            "/api/specs",
            get(plan_routes::spec::list).post(plan_routes::spec::create),
        )
        .route("/api/specs/{id}", get(plan_routes::spec::get))
        .route(
            "/api/specs/{id}/sections",
            post(plan_routes::spec::section_add),
        )
        .route(
            "/api/specs/{id}/sections/{sid}",
            patch(plan_routes::spec::section_update).delete(plan_routes::spec::section_rm),
        )
        .route("/api/specs/{id}/links", get(plan_routes::spec::link_list))
        .route("/api/efforts/{id}", get(plan_routes::spec::effort_show))
        .route(
            "/api/efforts/{id}/links",
            post(plan_routes::spec::effort_link),
        )
        .route(
            "/api/plans/{id}",
            get(plan_routes::get)
                .patch(plan_routes::update)
                .delete(plan_routes::delete_plan),
        )
        .route("/api/plans/{id}/archive", post(plan_routes::archive))
        .route("/api/plans/{id}/unarchive", post(plan_routes::unarchive))
        .route("/api/plans/{id}/pause", post(plan_routes::pause))
        .route(
            "/api/plans/{id}/pause/preview",
            get(plan_routes::pause_preview),
        )
        .route("/api/plans/{id}/resume", post(plan_routes::resume))
        .route(
            "/api/plans/{id}/resume/preview",
            get(plan_routes::resume_preview),
        )
        .route("/api/plans/{id}/mode", post(plan_routes::mode_set))
        .route("/api/plans/{id}/reconcile", get(plan_routes::reconcile))
        .merge(plan_routes::control_loop::router())
        .route("/api/plans/{id}/next", get(plan_routes::next))
        .route("/api/plans/{id}/attachments", get(plan_routes::attachment))
        .route("/api/plans/{id}/transfer", post(plan_routes::transfer))
        .route("/api/plans/{id}/disown", post(plan_routes::disown))
        .route("/api/plans/{id}/claim", post(plan_routes::claim))
        .route("/api/plans/{id}/ingest", post(plan_routes::ingest))
        .route("/api/plans/{id}/skills", get(plan_routes::skills_list))
        .route(
            "/api/plans/{id}/skills/{slug}",
            get(plan_routes::skills_show),
        )
        .route(
            "/api/plans/{id}/retrospective",
            post(plan_routes::retrospective_write),
        )
        .route("/api/plans/{id}/sections", post(plan_routes::section_add))
        .route(
            "/api/plans/{id}/sections/{sid}",
            patch(plan_routes::section_update).delete(plan_routes::section_rm),
        )
        .route("/api/plans/{id}/kinds", post(plan_routes::kinds_set))
        .route(
            "/api/plans/{id}/tag-facets",
            post(plan_routes::tag_facets_set),
        )
        .route("/api/plans/{id}/coverage", get(plan_routes::coverage))
        .route("/api/plans/{id}/groups", post(plan_routes::groups_set))
        .route("/api/plans/{id}/batch", post(plan_routes::batch::batch))
        .route(
            "/api/plans/{id}/handoffs",
            get(plan_routes::handoff::list).post(plan_routes::handoff::create),
        )
        .route(
            "/api/plans/{id}/handoffs/show",
            get(plan_routes::handoff::show),
        )
        .route("/api/plans/{id}/links", post(plan_routes::link))
        .route("/api/plans/{id}/links/{to}", delete(plan_routes::unlink))
        .route(
            "/api/plans/{id}/spec-links",
            post(plan_routes::spec::link_add),
        )
        .route(
            "/api/plans/{id}/spec-links/{spec_id}",
            delete(plan_routes::spec::link_rm),
        )
        .route("/api/plans/{id}/tasks", post(plan_routes::task_add))
        .route("/api/tasks", get(task_routes::list))
        .route("/api/tasks/{id}", get(task_routes::show))
        .route("/api/tasks/{id}/take", post(task_routes::take))
        .route("/api/tasks/{id}/drop", post(task_routes::drop_task))
        .route("/api/canvas/focus", post(task_routes::canvas_focus))
        .route(
            "/api/plans/{id}/tasks/{tid}",
            patch(plan_routes::task_update).delete(plan_routes::task_rm),
        )
        .route(
            "/api/plans/{id}/tasks/{tid}/close",
            post(plan_routes::task_close),
        )
        .route(
            "/api/plans/{id}/tasks/{tid}/assign",
            post(plan_routes::task_assign),
        )
        .route(
            "/api/plans/{id}/tasks/{tid}/attempts",
            get(plan_routes::task_attempts),
        )
        .route(
            "/api/plans/{id}/tasks/{tid}/attempts/{attempt_id}",
            get(plan_routes::task_attempt_show),
        )
        .route(
            "/api/plans/{id}/tasks/{tid}/respawn",
            post(plan_routes::task_respawn),
        )
        .route(
            "/api/plans/{id}/tasks/{tid}/adopt-checkpoint",
            post(plan_routes::task_adopt_checkpoint),
        )
        .route(
            "/api/plans/{id}/tasks/{tid}/milestones",
            post(plan_routes::milestone_add),
        )
        .route(
            "/api/plans/{id}/tasks/{tid}/milestones/{ms_id}",
            patch(plan_routes::milestone_update).delete(plan_routes::milestone_rm),
        )
        .route(
            "/api/plans/{id}/decision",
            post(plan_routes::gate_ask).get(plan_routes::gate_list),
        )
        .route(
            "/api/plans/{id}/decision/{gid}/answer",
            post(plan_routes::gate_answer),
        )
        .route(
            "/api/plans/{id}/decision/{gid}/escalate",
            post(plan_routes::gate_escalate),
        )
        .route("/api/plans/{id}/loops", post(plan_routes::loop_create))
        .route(
            "/api/plans/{id}/loops/{lid}",
            get(plan_routes::loop_status).patch(plan_routes::loop_set),
        )
        .route(
            "/api/plans/{id}/loops/{lid}/stop",
            post(plan_routes::loop_stop),
        )
        .route(
            "/api/plans/{id}/loops/{lid}/record",
            post(plan_routes::loop_record),
        )
        .route("/api/plans/{id}/layout", patch(plan_routes::layout_set))
        .route("/api/plans/{id}/notes", post(plan_routes::note_add))
        .route(
            "/api/plans/{id}/notes/{element_id}",
            patch(plan_routes::note_update).delete(plan_routes::element_rm),
        )
        .route("/api/plans/{id}/elements", post(plan_routes::element_add))
        .route(
            "/api/plans/{id}/elements/{element_id}",
            patch(plan_routes::element_update).delete(plan_routes::element_rm),
        )
}
