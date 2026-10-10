use super::*;

fn params(width: u32) -> ScreencastParams {
    ScreencastParams::new(Viewport {
        width,
        height: 720,
        dpr: 1.5,
    })
}

fn metadata() -> FrameMetadata {
    FrameMetadata::still(Viewport::default(), 1_728_381_234.5)
}

#[test]
fn browser_screencast_pacer_spaces_acks_by_interval() {
    let start = Instant::now();
    let mut pacer = AckPacer::new(ACK_INTERVAL);
    assert_eq!(pacer.ack_due(start), start);
    pacer.acked(start);
    let fast = start + Duration::from_millis(10);
    assert_eq!(pacer.ack_due(fast), start + ACK_INTERVAL);
    let slow = start + Duration::from_millis(200);
    assert_eq!(pacer.ack_due(slow), slow);
    pacer.reset();
    assert_eq!(pacer.ack_due(fast), fast);
}

#[test]
fn browser_screencast_pacer_caps_a_flooding_page_near_twelve_fps() {
    let start = Instant::now();
    let mut pacer = AckPacer::new(ACK_INTERVAL);
    let mut now = start;
    let mut acks = 0;
    loop {
        let due = pacer.ack_due(now);
        if due >= start + Duration::from_secs(1) {
            break;
        }
        pacer.acked(due);
        acks += 1;
        now = due + Duration::from_millis(1);
    }
    assert!((12..=13).contains(&acks), "{acks}");
}

#[test]
fn browser_screencast_viewer_count_starts_on_first_and_stops_on_last() {
    let mut viewers = ViewerCount::default();
    assert_eq!(viewers.join(), Some(CastTransition::Start));
    assert_eq!(viewers.join(), None);
    assert_eq!(viewers.leave(), None);
    assert_eq!(viewers.leave(), Some(CastTransition::Stop));
    assert_eq!(viewers.leave(), None);
    assert_eq!(viewers.count(), 0);
}

#[test]
fn browser_screencast_skipped_counts_missed_frames() {
    assert_eq!(skipped_between(0, 1), 0);
    assert_eq!(skipped_between(3, 7), 3);
    assert_eq!(skipped_between(7, 7), 0);
}

#[test]
fn browser_screencast_parses_frame_with_float_second_timestamp() {
    let raw = parse_frame(&json!({
        "data": "/9j/4AAQ",
        "sessionId": 17,
        "metadata": {
            "offsetTop": 0, "pageScaleFactor": 1, "deviceWidth": 1280, "deviceHeight": 720,
            "scrollOffsetX": 0, "scrollOffsetY": 412.5, "timestamp": 1728381234.567891
        }
    }))
    .unwrap();
    assert_eq!(raw.ack_id, 17);
    assert_eq!(raw.metadata.scroll_offset_y, 412.5);
    assert_eq!(raw.metadata.timestamp, Some(1728381234.567891));
    let latency = raw.metadata.latency_ms(1728381234.667891).unwrap();
    assert!((latency - 100.0).abs() < 0.01, "{latency}");
    assert!(parse_frame(&json!({"data": "x", "metadata": {}})).is_err());
    assert!(parse_frame(&json!({"sessionId": 1, "metadata": {}})).is_err());
    assert!(parse_frame(&json!({"sessionId": 1, "data": "x"})).is_err());
}

#[test]
fn browser_screencast_start_params_use_device_pixels() {
    assert_eq!(
        params(1280).start_json(),
        json!({"format": "jpeg", "quality": 90, "maxWidth": 1920, "maxHeight": 1080, "everyNthFrame": 1})
    );
}

#[test]
fn browser_screencast_hub_drives_start_stop_by_viewers() {
    let hub = FrameHub::new(params(1280));
    let (driver, mut commands) = mpsc::unbounded_channel();
    hub.bind(driver);
    let first = hub.subscribe();
    let second = hub.subscribe();
    assert_eq!(commands.try_recv(), Ok(DriverCommand::Start(params(1280))));
    assert!(commands.try_recv().is_err());
    hub.set_params(params(1600));
    assert_eq!(
        commands.try_recv(),
        Ok(DriverCommand::Restart(params(1600)))
    );
    drop(first);
    assert!(commands.try_recv().is_err());
    drop(second);
    assert_eq!(commands.try_recv(), Ok(DriverCommand::Stop));
    hub.set_params(params(800));
    assert!(commands.try_recv().is_err());
}

#[test]
fn browser_screencast_rebind_restarts_for_existing_viewers_and_drops_old_frames() {
    let hub = FrameHub::new(params(1280));
    let (old_driver, _old) = mpsc::unbounded_channel();
    let old_generation = hub.bind(old_driver);
    let _viewer = hub.subscribe();
    let (new_driver, mut commands) = mpsc::unbounded_channel();
    let generation = hub.bind(new_driver);
    assert_eq!(commands.try_recv(), Ok(DriverCommand::Start(params(1280))));
    assert!(!hub.publish(old_generation, vec![1], metadata(), false));
    assert!(hub.publish(generation, vec![2], metadata(), false));
    assert_eq!(hub.latest().unwrap().jpeg, vec![2]);
    hub.unbind(old_generation);
    hub.set_params(params(900));
    assert_eq!(commands.try_recv(), Ok(DriverCommand::Restart(params(900))));
}

#[tokio::test]
async fn browser_screencast_backlogged_viewer_gets_latest_frame_only() {
    let hub = FrameHub::new(params(1280));
    let (driver, _commands) = mpsc::unbounded_channel();
    let generation = hub.bind(driver);
    let mut viewer = hub.subscribe();
    for byte in 1..=4u8 {
        hub.publish(generation, vec![byte], metadata(), false);
    }
    let delivery = viewer.next().await.unwrap();
    assert_eq!(delivery.frame.jpeg, vec![4]);
    assert_eq!(delivery.skipped, 3);
    hub.publish(generation, vec![5], metadata(), true);
    let delivery = viewer.next().await.unwrap();
    assert_eq!(delivery.skipped, 0);
    assert!(delivery.frame.still);
}

#[test]
fn browser_screencast_capture_hold_drops_frames_sized_unlike_the_viewport() {
    let hub = FrameHub::new(params(1280));
    let (driver, _commands) = mpsc::unbounded_channel();
    let generation = hub.bind(driver);
    let page = FrameMetadata::still(params(1280).viewport, 1.0);
    let zoomed = FrameMetadata {
        device_width: 420.0,
        device_height: 180.0,
        ..page.clone()
    };
    let hold = hub.hold_for_capture();
    assert!(!hub.publish(generation, vec![1], zoomed.clone(), false));
    assert!(hub.publish(generation, vec![2], page.clone(), false));
    assert!(
        hub.publish(generation, vec![3], zoomed.clone(), true),
        "a still is ours, never re-rendered"
    );
    assert_eq!(hub.latest().unwrap().jpeg, vec![3]);
    drop(hold);
    assert!(
        hub.publish(generation, vec![4], zoomed, false),
        "the guard ends with the capture"
    );
    assert_eq!(hub.latest().unwrap().jpeg, vec![4]);
}

#[tokio::test]
async fn browser_screencast_closed_hub_ends_subscriptions() {
    let hub = FrameHub::new(params(1280));
    let mut viewer = hub.subscribe();
    hub.close();
    assert!(viewer.next().await.is_none());
    assert!(!hub.publish(0, vec![1], metadata(), false));
}

#[test]
fn browser_screencast_hub_tags_still_failures_as_preview_paused() {
    let hub = FrameHub::new(params(1280));
    let mut notices = hub.notices();
    hub.notice("screencast could not start: x");
    hub.preview_paused("no frame: window not rendering (timeout)");
    assert_eq!(
        notices.try_recv().unwrap(),
        HubNotice {
            kind: HubNoticeKind::Plain,
            text: "screencast could not start: x".into()
        }
    );
    assert_eq!(
        notices.try_recv().unwrap(),
        HubNotice {
            kind: HubNoticeKind::PreviewPaused,
            text: "no frame: window not rendering (timeout)".into()
        }
    );
}
