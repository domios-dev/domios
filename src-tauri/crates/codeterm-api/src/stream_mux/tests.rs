use super::frames::*;
use super::*;

fn frames_of(splitter: &mut SseSplitter, input: &[u8]) -> Vec<SseFrame> {
    splitter.push(input).expect("within cap")
}

#[test]
fn splitter_reassembles_frames_across_chunks_and_line_endings() {
    let mut splitter = SseSplitter::new(1024);
    assert!(frames_of(&mut splitter, b"data: {\"a\":").is_empty());
    let frames = frames_of(
        &mut splitter,
        b"1}\n\nevent: named\r\ndata: x\r\ndata: y\r\n\r\n: keep-alive\n\n",
    );
    assert_eq!(
        frames,
        vec![
            SseFrame {
                event: None,
                data: Some("{\"a\":1}".into())
            },
            SseFrame {
                event: Some("named".into()),
                data: Some("x\ny".into())
            },
            SseFrame {
                event: None,
                data: None
            },
        ]
    );
}

#[test]
fn splitter_refuses_a_frame_over_its_cap() {
    let mut splitter = SseSplitter::new(8);
    assert_eq!(splitter.push(b"data: 123456"), Err(9));
}

#[test]
fn encoded_frames_tag_the_channel_and_stay_one_sse_line() {
    let raw = encode_frame(
        "c1",
        &SseFrame {
            event: None,
            data: Some("{\"seq\":3,\"boot\":\"b\"}".into()),
        },
    );
    assert_eq!(
        &raw[..],
        b"data: {\"channel\":\"c1\",\"data\":{\"seq\":3,\"boot\":\"b\"}}\n\n"
    );
    let multi = encode_frame(
        "c1",
        &SseFrame {
            event: Some("tick".into()),
            data: Some("{\"a\":\n1}".into()),
        },
    );
    assert_eq!(
        &multi[..],
        b"data: {\"channel\":\"c1\",\"event\":\"tick\",\"data\":{\"a\":1}}\n\n"
    );
    let text = encode_frame(
        "c1",
        &SseFrame {
            event: Some("message".into()),
            data: Some("plain\ntext".into()),
        },
    );
    assert_eq!(
        &text[..],
        b"data: {\"channel\":\"c1\",\"data\":\"plain\\ntext\"}\n\n"
    );
    let keep_alive = encode_frame(
        "c1",
        &SseFrame {
            event: None,
            data: None,
        },
    );
    assert_eq!(
        &keep_alive[..],
        b"data: {\"channel\":\"c1\",\"activity\":true}\n\n"
    );
    assert_eq!(
        &encode_open("c1", Some("text/event-stream"))[..],
        b"data: {\"channel\":\"c1\",\"open\":true,\"content_type\":\"text/event-stream\"}\n\n"
    );
    assert_eq!(
        &encode_end("c1", &ChannelEnd::Http { status: 404, body: "gone".into() })[..],
        b"data: {\"channel\":\"c1\",\"end\":{\"reason\":\"http\",\"status\":404,\"body\":\"gone\"}}\n\n"
    );
}

#[test]
fn only_stream_paths_become_channels() {
    assert!(matches!(
        classify_channel_path("/api/mesh/relay-stream?peer=p&path=%2Fapi%2Fevents%2Fstream"),
        Ok(MuxTarget::Relay(_))
    ));
    assert!(matches!(
        classify_channel_path("/api/output/stream?since=abc"),
        Ok(MuxTarget::LocalOutput(_))
    ));
    for refused in [
        "/api/mux/state",
        "//evil/api/output/stream",
        "api/output/stream",
        "/api\\output/stream",
    ] {
        assert!(classify_channel_path(refused).is_err(), "{refused}");
    }
    assert!(valid_mux_id("a1-_B"));
    assert!(!valid_mux_id(""));
    assert!(!valid_mux_id("a/b"));
    assert!(!valid_mux_id(&"x".repeat(65)));
}

fn app_state(fixture: &codeterm_service_core::service::test_fixture::ServiceFixture) -> AppState {
    let config = codeterm_service_core::config::schema::AppConfig::default();
    let mut mesh = codeterm_service_core::mesh::MeshManager::new("stream-mux".into(), 0);
    mesh.disk_persist = false;
    mesh.paired_peers.clear();
    AppState {
        frontend_dev_url: None,
        svc: std::sync::Arc::new(fixture.service.clone()),
        cron: std::sync::Arc::new(parking_lot::Mutex::new(
            codeterm_service_core::cron::store::CronStore::new_with_path(
                fixture.temp.path().join("stream-mux-crons.json"),
            ),
        )),
        cron_wake: std::sync::Arc::new(std::sync::Condvar::new()),
        api_port: 0,
        update_state: std::sync::Arc::new(parking_lot::Mutex::new(codeterm_update::UpdateState {
            install_method: codeterm_update::InstallMethod::Unknown,
            last_check: None,
            last_check_time: None,
            channel: config.updater.channel,
            phase: codeterm_update::UpdatePhase::Idle,
            last_error: None,
            phase_changed_at: None,
            downloaded_bytes: 0,
            download_total_bytes: None,
        })),
        config: std::sync::Arc::new(parking_lot::Mutex::new(config)),
        mesh: std::sync::Arc::new(parking_lot::Mutex::new(mesh)),
        keepawake: std::sync::Arc::new(parking_lot::Mutex::new(None)),
        resize_owner: std::sync::Arc::new(parking_lot::Mutex::new(Default::default())),
        rest_api: std::sync::Arc::new(codeterm_service_core::startup::RestApiControl::new()),
        test_clock: None,
        events: fixture.service.events.clone(),
        web_dist: None,
    }
}

async fn next_frame(
    body: &mut (impl futures_core::Stream<Item = Result<Bytes, axum::Error>> + Unpin),
) -> String {
    let bytes = body.next().await.expect("mux frame").expect("mux bytes");
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn channel_refusal_arrives_tagged_and_the_session_closes_with_its_stream() {
    let (mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
    let state = app_state(&fixture);
    let session = "mux-test-session";
    let response = stream_mux_open(Query(MuxSessionQuery {
        session: session.into(),
    }))
    .await
    .expect("session opens");
    assert_eq!(
        stream_mux_open(Query(MuxSessionQuery {
            session: session.into()
        }))
        .await
        .unwrap_err()
        .0,
        StatusCode::CONFLICT
    );
    let mut body = response.into_body().into_data_stream();
    assert_eq!(next_frame(&mut body).await, ": mux\n\n");

    let open = |channel: &str| {
        stream_mux_open_channel(
            AxumState(state.clone()),
            HeaderMap::new(),
            AxumPath(session.into()),
            Json(MuxChannelOpen {
                channel: channel.into(),
                path: "/api/mesh/relay-stream?peer=ghost&path=%2Fapi%2Fevents%2Fstream".into(),
            }),
        )
    };
    let accepted = open("chan-a").await.expect("channel accepted");
    assert_eq!(accepted.0["ok"], true);
    let end = next_frame(&mut body).await;
    assert!(
        end.starts_with(
            "data: {\"channel\":\"chan-a\",\"end\":{\"reason\":\"http\",\"status\":404"
        ),
        "{end}"
    );

    let closed = stream_mux_close_channel(AxumPath((session.into(), "chan-a".into()))).await;
    assert_eq!(closed.0["closed"], false);

    drop(body);
    assert_eq!(open("chan-b").await.unwrap_err().0, StatusCode::GONE);
    stream_mux_open(Query(MuxSessionQuery {
        session: session.into(),
    }))
    .await
    .expect("a released session id can reopen");
}
