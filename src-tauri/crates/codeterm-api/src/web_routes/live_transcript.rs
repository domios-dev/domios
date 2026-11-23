use axum::{
    extract::{DefaultBodyLimit, Path, Query, Request, State},
    http::{header, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response as AxumResponse},
    routing::{get, post},
    Extension, Json, Router,
};
use codeterm_service_core::service::{
    dictation::live_capture::LiveCaptureStart,
    live_transcript::{
        CursorStart, TranscriptBinding, TranscriptPosition, TranscriptRange, TranscriptStore,
        TranscriptTrigger,
    },
    ServiceContext,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{
    auth::{AuthenticatedOrigin, CallerIdentity},
    routes::AppState,
};

type Refusal = (StatusCode, String);
type Response = Result<Json<Value>, Refusal>;

pub(crate) fn router() -> Router<AppState> {
    let owner = Router::new()
        .route("/api/live-transcript/{tab}/status", get(status))
        .route("/api/live-transcript/{tab}/overview", get(overview))
        .route("/api/live-transcript/{tab}/history", get(history))
        .route("/api/live-transcript/{tab}/search", get(search))
        .route("/api/live-transcript/{tab}/events", get(events))
        .route("/api/live-transcript/{tab}/epochs/{epoch}", get(epoch))
        .route("/api/live-transcript/{tab}/attach", post(attach))
        .route("/api/live-transcript/{tab}/pause", post(pause))
        .route("/api/live-transcript/{tab}/resume", post(resume))
        .route("/api/live-transcript/{tab}/route", post(route))
        .route_layer(middleware::from_fn(owner_only));
    Router::new()
        .merge(owner)
        .route("/api/live-transcript/readiness", get(readiness))
        .route("/api/live-transcript/{tab}/read", get(read))
        .route("/api/live-transcript/{tab}/ack", post(ack))
        .route("/api/live-transcript/{tab}/mark-seen", post(mark_seen))
        .route_layer(DefaultBodyLimit::max(2048))
        .route_layer(middleware::from_fn(local_transcript))
}

async fn local_transcript(request: Request, next: Next) -> AxumResponse {
    let direct = matches!(
        (
            request.extensions().get::<CallerIdentity>(),
            request.extensions().get::<AuthenticatedOrigin>()
        ),
        (
            Some(CallerIdentity::Local),
            Some(AuthenticatedOrigin::Owner)
        ) | (
            Some(CallerIdentity::Tab(_)),
            Some(AuthenticatedOrigin::Agent)
        )
    );
    if !direct {
        return refuse(
            StatusCode::FORBIDDEN,
            "Transcript requires a direct owner or tab credential",
        )
        .into_response();
    }
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

async fn owner_only(request: Request, next: Next) -> AxumResponse {
    if !matches!(
        request.extensions().get::<CallerIdentity>(),
        Some(CallerIdentity::Local)
    ) {
        return refuse(
            StatusCode::FORBIDDEN,
            "Transcript operation requires the local owner credential",
        )
        .into_response();
    }
    next.run(request).await
}

async fn blocking<T: Serialize + Send + 'static>(
    action: impl FnOnce() -> Result<T, Refusal> + Send + 'static,
) -> Response {
    tokio::task::spawn_blocking(move || {
        serde_json::to_value(action()?).map(Json).map_err(|_| {
            refuse(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Transcript response failed",
            )
        })
    })
    .await
    .map_err(|_| {
        refuse(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Transcript worker stopped",
        )
    })?
}

fn refuse(code: StatusCode, message: impl Into<String>) -> Refusal {
    (code, message.into())
}

fn conflict(message: impl ToString) -> Refusal {
    refuse(StatusCode::CONFLICT, message.to_string())
}

fn with_consumer<T>(
    svc: &ServiceContext,
    caller: CallerIdentity,
    owner: &str,
    revision: Option<u64>,
    use_binding: impl FnOnce(&TranscriptStore, &TranscriptBinding) -> Result<T, Refusal>,
) -> Result<T, Refusal> {
    match caller {
        CallerIdentity::Local => {
            let store = svc.open_live_transcript(owner).map_err(conflict)?;
            let binding = store.binding();
            if revision.is_some_and(|revision| revision != binding.revision) {
                return Err(conflict("Stale transcript binding revision"));
            }
            use_binding(&store, &binding)
        }
        CallerIdentity::Tab(caller) => {
            let denied = || {
                refuse(
                    StatusCode::FORBIDDEN,
                    "Current transcript consumer and revision required",
                )
            };
            let revision = revision.ok_or_else(denied)?;
            svc.with_live_transcript_consumer(owner, &caller, revision, use_binding)
                .map_err(|_| denied())?
        }
        _ => Err(refuse(StatusCode::FORBIDDEN, "Transcript access denied")),
    }
}

async fn status(State(state): State<AppState>, Path(tab): Path<String>) -> Response {
    let mut status = blocking({
        let svc = state.svc.clone();
        let tab = tab.clone();
        move || svc.live_transcript_status(&tab).map_err(conflict)
    })
    .await?;
    status.0["asr"] = json!(state.svc.live_capture_readiness().await);
    status.0["captureActive"] = json!(state.svc.live_capture_sessions().active_for_owner(&tab));
    Ok(status)
}

async fn readiness(
    State(state): State<AppState>,
    Extension(caller): Extension<CallerIdentity>,
) -> Response {
    let mut readiness = tokio::task::spawn_blocking({
        let svc = state.svc.clone();
        move || {
            let allowed = match caller {
                CallerIdentity::Local => true,
                CallerIdentity::Tab(caller) => {
                    svc.mux.read().get_tab(&caller.tab_id).is_some_and(|tab| {
                        tab.session_binding_nonce.as_deref() == Some(caller.binding_nonce.as_str())
                            && codeterm_service_core::service::general_agent::is_general_agent(tab)
                    })
                }
                _ => false,
            };
            if !allowed {
                return Err(refuse(
                    StatusCode::FORBIDDEN,
                    "Transcript setup readiness requires the local owner or General Agent",
                ));
            }
            Ok(svc.live_transcript_setup())
        }
    })
    .await
    .map_err(|_| {
        refuse(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Transcript worker stopped",
        )
    })??;
    readiness["asr"] = json!(state.svc.live_capture_readiness().await);
    Ok(Json(readiness))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Read {
    revision: Option<u64>,
    from_seq: Option<u64>,
    through_seq: Option<u64>,
    char_offset: Option<usize>,
    max_chars: Option<usize>,
}

async fn read(
    State(state): State<AppState>,
    Path(tab): Path<String>,
    Extension(caller): Extension<CallerIdentity>,
    Query(read): Query<Read>,
) -> Response {
    blocking(move || {
        with_consumer(&state.svc, caller, &tab, read.revision, |store, binding| {
            let response = if let Some(through) = read.through_seq {
                let cursor = store.cursor(binding).map_err(conflict)?;
                let from = read.from_seq.unwrap_or(cursor.acked_seq.saturating_add(1));
                if from <= cursor.acked_seq || from > through || through > store.committed_seq() {
                    return Err(refuse(
                        StatusCode::BAD_REQUEST,
                        "Invalid committed transcript range",
                    ));
                }
                let limit = through.saturating_sub(from).saturating_add(1).min(8) as usize;
                let mut page = store.history(from, limit).map_err(conflict)?;
                page.rows.retain(|row| row.seq <= through);
                page.gaps.retain(|gap| gap.from_seq <= through);
                for gap in &mut page.gaps {
                    gap.to_seq = gap.to_seq.min(through);
                }
                page.next_from_seq = page.next_from_seq.filter(|next| *next <= through);
                json!(page)
            } else {
                let position = read.from_seq.map(|seq| TranscriptPosition {
                    seq,
                    char_offset: read.char_offset.unwrap_or_default(),
                });
                json!(store
                    .read_page(binding, position, read.max_chars.unwrap_or(3000), None)
                    .map_err(conflict)?)
            };
            Ok(response)
        })
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Advance {
    revision: u64,
    through_seq: u64,
}

async fn ack(
    State(state): State<AppState>,
    Path(tab): Path<String>,
    Extension(caller): Extension<CallerIdentity>,
    Json(advance): Json<Advance>,
) -> Response {
    advance_cursor(state, tab, caller, advance, true).await
}

async fn mark_seen(
    State(state): State<AppState>,
    Path(tab): Path<String>,
    Extension(caller): Extension<CallerIdentity>,
    Json(advance): Json<Advance>,
) -> Response {
    advance_cursor(state, tab, caller, advance, false).await
}

async fn advance_cursor(
    state: AppState,
    tab: String,
    caller: CallerIdentity,
    advance: Advance,
    ack: bool,
) -> Response {
    blocking(move || {
        let cursor = with_consumer(
            &state.svc,
            caller,
            &tab,
            Some(advance.revision),
            |store, binding| {
                let cursor = if ack {
                    let cursor = store.ack(binding, advance.through_seq).map_err(conflict)?;
                    state.svc.schedule_live_transcript_retention(store.clone());
                    cursor
                } else {
                    store
                        .mark_seen(binding, advance.through_seq)
                        .map_err(conflict)?
                };
                Ok(cursor)
            },
        )?;
        state.svc.live_transcript_cursor_changed(&tab, ack);
        Ok(cursor)
    })
    .await
}

async fn overview(State(state): State<AppState>, Path(tab): Path<String>) -> Response {
    blocking(move || {
        let status = state.svc.live_transcript_status(&tab).map_err(conflict)?;
        let store = state.svc.open_live_transcript(&tab).map_err(conflict)?;
        let cursor = store.cursor(&status.binding).ok();
        let forwards = store.forwards().map_err(conflict)?;
        Ok(json!({
            "status": status,
            "cursor": cursor,
            "rawBytes": store.raw_bytes(),
            "forwards": forwards,
        }))
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Page {
    from_seq: Option<u64>,
    limit: Option<usize>,
}

async fn history(
    State(state): State<AppState>,
    Path(tab): Path<String>,
    Query(page): Query<Page>,
) -> Response {
    blocking(move || {
        state
            .svc
            .open_live_transcript(&tab)
            .map_err(conflict)?
            .history(page.from_seq.unwrap_or(1), page.limit.unwrap_or(16))
            .map_err(conflict)
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Search {
    query: String,
    from_seq: Option<u64>,
    limit: Option<usize>,
}

async fn search(
    State(state): State<AppState>,
    Path(tab): Path<String>,
    Query(search): Query<Search>,
) -> Response {
    blocking(move || {
        state
            .svc
            .open_live_transcript(&tab)
            .map_err(conflict)?
            .search(
                &search.query,
                search.from_seq.unwrap_or(1),
                search.limit.unwrap_or(16),
            )
            .map_err(conflict)
    })
    .await
}

async fn events(
    State(state): State<AppState>,
    Path(tab): Path<String>,
    Query(page): Query<Page>,
) -> Response {
    blocking(move || {
        state.svc.live_transcript_status(&tab).map_err(conflict)?;
        state
            .svc
            .open_live_transcript(&tab)
            .map_err(conflict)?
            .routed_actions(page.from_seq.unwrap_or(0), page.limit.unwrap_or(20))
            .map_err(conflict)
    })
    .await
}

#[derive(Deserialize)]
struct EpochPage {
    offset: Option<usize>,
    limit: Option<usize>,
}

async fn epoch(
    State(state): State<AppState>,
    Path((tab, epoch)): Path<(String, String)>,
    Query(page): Query<EpochPage>,
) -> Response {
    blocking(move || {
        let store = state.svc.open_live_transcript(&tab).map_err(conflict)?;
        let (rows, next_offset) = state
            .svc
            .attached_transcript_history(
                &store,
                &tab,
                &epoch,
                page.offset.unwrap_or(0),
                page.limit.unwrap_or(20),
            )
            .map_err(conflict)?;
        Ok(json!({ "rows": rows, "nextOffset": next_offset }))
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Attach {
    external_tab: Option<String>,
    trigger: Option<TranscriptTrigger>,
    start: Option<CursorStart>,
    provider: Option<String>,
    model: Option<String>,
    configure: Option<bool>,
    default_mic_tap: Option<bool>,
    #[serde(default)]
    mode_only: bool,
}

async fn attach(
    State(state): State<AppState>,
    Path(tab): Path<String>,
    Json(request): Json<Attach>,
) -> Response {
    blocking(move || {
        if request.mode_only
            && (request.external_tab.is_some()
                || request.start.is_some()
                || (request.trigger.is_none() && request.default_mic_tap.is_none()))
        {
            return Err(refuse(
                StatusCode::BAD_REQUEST,
                "Mode-only changes require a trigger or microphone default",
            ));
        }
        state
            .svc
            .configure_live_transcript(
                &tab,
                request.configure.unwrap_or(false),
                request.provider,
                request.model,
                request.trigger,
                request.default_mic_tap,
            )
            .map_err(conflict)?;
        if request.mode_only {
            if let Some(trigger) = request.trigger {
                state
                    .svc
                    .set_live_transcript_trigger(&tab, trigger)
                    .map_err(conflict)?;
            }
        } else {
            state
                .svc
                .update_live_transcript_binding(
                    &tab,
                    request.external_tab.as_deref(),
                    request.start.unwrap_or(CursorStart::Latest),
                    request.trigger,
                )
                .map_err(conflict)?;
        }
        let mut status = json!(state.svc.live_transcript_status(&tab).map_err(conflict)?);
        if !request.mode_only {
            for field in ["ownerTabId", "revision", "target"] {
                status[field] = status["binding"][field].clone();
            }
        }
        Ok(status)
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Pause {
    client: Uuid,
    lease: Uuid,
    sequence: u32,
}

async fn pause(
    State(state): State<AppState>,
    Path(tab): Path<String>,
    Json(pause): Json<Pause>,
) -> Response {
    state
        .svc
        .live_capture_sessions()
        .pause_owner(&tab, pause.client, pause.lease, pause.sequence)
        .await
        .map(|value| Json(json!(value)))
        .map_err(conflict)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Resume {
    client: Uuid,
}

async fn resume(
    State(state): State<AppState>,
    Path(tab): Path<String>,
    Json(resume): Json<Resume>,
) -> Response {
    let result = state
        .svc
        .start_live_capture(
            resume.client,
            LiveCaptureStart {
                tab_id: tab,
                resume: true,
            },
        )
        .await;
    Ok(Json(json!(result)))
}

async fn route(
    State(state): State<AppState>,
    Path(tab): Path<String>,
    Json(range): Json<TranscriptRange>,
) -> Response {
    blocking(move || {
        state
            .svc
            .route_live_transcript_manually(&tab, range)
            .map_err(conflict)?;
        Ok(json!({ "accepted": true }))
    })
    .await
}
