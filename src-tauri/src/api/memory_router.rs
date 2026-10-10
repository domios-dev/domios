use axum::{
    routing::{delete, get, post},
    Router,
};

use super::{mem_routes, routes::AppState};

pub(super) fn memory_routes(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/api/mem/search", get(mem_routes::mem_search))
        .route(
            "/api/supporter/connections",
            get(mem_routes::decision::connections_list)
                .post(mem_routes::decision::connections_connect),
        )
        .route(
            "/api/supporter/connections/{adapter}",
            axum::routing::delete(mem_routes::decision::connections_disconnect),
        )
        .route(
            "/api/supporter/connections/{adapter}/activate",
            post(mem_routes::decision::connections_activate),
        )
        .route(
            "/api/supporter/connections/{adapter}/model",
            axum::routing::put(mem_routes::decision::connections_model),
        )
        .route(
            "/api/supporter/connections/{adapter}/token",
            post(mem_routes::decision::connections_token),
        )
        .route(
            "/api/supporter/token",
            get(mem_routes::decision::supporter_token_status)
                .post(mem_routes::decision::supporter_token_set)
                .delete(mem_routes::decision::supporter_token_clear),
        )
        .route(
            "/api/supporter/activity",
            get(mem_routes::decision::supporter_feed),
        )
        .route(
            "/api/supporter/status",
            get(mem_routes::decision::supporter_status),
        )
        .route(
            "/api/supporter/models",
            get(mem_routes::decision::supporter_models),
        )
        .route(
            "/api/supporter/use",
            post(mem_routes::decision::supporter_use),
        )
        .route("/api/mem/items", post(mem_routes::mem_save))
        .route("/api/mem/pinned", get(mem_routes::mem_pinned_list))
        .route(
            "/api/mem/items/{id}",
            get(mem_routes::mem_get)
                .delete(mem_routes::mem_delete)
                .patch(mem_routes::mem_update),
        )
        .route("/api/mem/items/{id}/history", get(mem_routes::mem_history))
        .route("/api/mem/items/{id}/append", post(mem_routes::mem_append))
        .route("/api/mem/items/{id}/archive", post(mem_routes::mem_archive))
        .route("/api/mem/items/{id}/restore", post(mem_routes::mem_restore))
        .route("/api/mem/merge", post(mem_routes::mem_merge))
        .route(
            "/api/mem/area/merge",
            post(mem_routes::area_merge::mem_area_merge),
        )
        .route("/api/mem/taxonomy", get(mem_routes::mem_taxonomy))
        .route("/api/mem/tail", get(mem_routes::mem_tail))
        .route(
            "/api/mem/forget/{area}",
            delete(mem_routes::mem_forget_area),
        )
        .route("/api/mem/wake-up", get(mem_routes::mem_wake_up))
        .route("/api/mem/stats", get(mem_routes::mem_stats))
        .route("/api/mem/reembed", post(mem_routes::mem_reembed))
        .route(
            "/api/mem/graph/entities",
            post(mem_routes::mem_graph_create),
        )
        .route("/api/mem/graph/relate", post(mem_routes::mem_graph_relate))
        .route("/api/mem/graph/related", get(mem_routes::mem_graph_related))
        .route(
            "/api/mem/graph/invalidate",
            post(mem_routes::mem_graph_invalidate),
        )
        .route(
            "/api/mem/graph/timeline",
            get(mem_routes::mem_graph_timeline),
        )
        .route(
            "/api/mem/graph/traverse",
            get(mem_routes::mem_graph_traverse),
        )
        .route("/api/mem/graph/path", get(mem_routes::mem_graph_path))
        .route("/api/mem/graph/tunnels", get(mem_routes::mem_graph_tunnels))
        .route("/api/mem/graph/alias", post(mem_routes::mem_graph_alias))
        .route(
            "/api/mem/graph/repair/find-relations",
            post(mem_routes::mem_graph_repair_find_relations),
        )
        .route(
            "/api/mem/graph/repair/delete-relations",
            post(mem_routes::mem_graph_repair_delete_relations),
        )
        .route(
            "/api/mem/graph/repair/merge-relations",
            post(mem_routes::mem_graph_repair_merge_relations),
        )
        .route(
            "/api/mem/graph/repair/find-entities",
            post(mem_routes::mem_graph_repair_find_entities),
        )
        .route(
            "/api/mem/graph/repair/augment-entity",
            post(mem_routes::mem_graph_repair_augment_entity),
        )
        .route(
            "/api/mem/graph/repair/delete-entity",
            post(mem_routes::mem_graph_repair_delete_entity),
        )
        .route(
            "/api/mem/graph/repair/orphan-vectors",
            post(mem_routes::mem_graph_repair_orphan_vectors),
        )
        .route(
            "/api/mem/graph/repair/prune-thin-vectors",
            post(mem_routes::mem_graph_prune_thin_vectors),
        )
        .route(
            "/api/mem/graph/repair/collapse-synonym-edges",
            post(mem_routes::mem_graph_collapse_synonym_edges),
        )
        .route(
            "/api/mem/graph/repair/prune-dream-hub-edges",
            post(mem_routes::mem_graph_prune_dream_hub_edges),
        )
        .route(
            "/api/mem/graph/repair/orphan-blobs",
            post(mem_routes::mem_graph_repair_orphan_blobs),
        )
        .route("/api/mem/learn", post(mem_routes::mem_learn))
        .route("/api/mem/promote/{id}", post(mem_routes::mem_promote))
        .route(
            "/api/mem/check-duplicate",
            post(mem_routes::mem_check_duplicate),
        )
        .route("/api/mem/dream", post(mem_routes::mem_dream))
        .route("/api/mem/dream/diary", get(mem_routes::mem_dream_diary))
        .route(
            "/api/mem/extract-pending",
            post(mem_routes::mem_extract_pending),
        )
        .route("/api/mem/dream/report", get(mem_routes::mem_dream_report))
        .route("/api/mem/graph/full", get(mem_routes::mem_graph_full))
        .route("/api/mem/graph/labels", get(mem_routes::mem_graph_labels))
        .route("/api/mem/graph/edges", get(mem_routes::mem_graph_edges))
        .route(
            "/api/mem/graph/layout/rebuild",
            post(mem_routes::mem_graph_layout_rebuild),
        )
        .route(
            "/api/mem/pinned-positions",
            get(mem_routes::mem_pinned_positions_list),
        )
        .route(
            "/api/mem/pinned-positions/{mem_id}",
            axum::routing::patch(mem_routes::mem_pinned_positions_set)
                .delete(mem_routes::mem_pinned_positions_delete),
        )
        .route(
            "/api/mem/activity/recent",
            get(mem_routes::mem_activity_recent),
        )
        .route(
            "/api/mem/calibration/sweep",
            post(mem_routes::mem_calibration_sweep),
        )
        .route(
            "/api/mem/calibration/log",
            get(mem_routes::mem_calibration_log),
        )
        .route(
            "/api/mem/calibration/stale",
            post(mem_routes::mem_calibration_stale),
        )
        .route(
            "/api/mem/workers",
            get(mem_routes::workers_list).post(mem_routes::workers_spawn),
        )
        .route(
            "/api/mem/workers/{id}",
            get(mem_routes::workers_get).delete(mem_routes::workers_kill),
        )
        .route(
            "/api/mem/workers/{id}/progress",
            post(mem_routes::workers_progress),
        )
        .route(
            "/api/mem/workers/{id}/report",
            post(mem_routes::workers_report),
        )
        .route("/api/mem/workers/{id}/log", get(mem_routes::workers_log))
        .route("/api/mem/sync", post(mem_routes::mem_sync))
        .route("/api/mem/sync/status", get(mem_routes::mem_sync_status))
}
