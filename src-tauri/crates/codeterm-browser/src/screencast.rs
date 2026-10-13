use super::cdp::PageChannel;
use super::viewport::Viewport;
use base64::Engine;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{broadcast, mpsc, watch};

/// ~12 fps: Chrome sends the next frame only after an ack, so pacing acks caps the rate.
pub const ACK_INTERVAL: Duration = Duration::from_millis(80);
pub const DEFAULT_QUALITY: u8 = 90;
pub const STILL_DEADLINE: Duration = Duration::from_secs(3);

#[derive(Clone, Debug)]
pub struct AckPacer {
    interval: Duration,
    last_ack: Option<Instant>,
}

impl AckPacer {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            last_ack: None,
        }
    }

    pub fn ack_due(&self, frame_at: Instant) -> Instant {
        match self.last_ack {
            Some(last) => frame_at.max(last + self.interval),
            None => frame_at,
        }
    }

    pub fn acked(&mut self, at: Instant) {
        self.last_ack = Some(at);
    }

    pub fn reset(&mut self) {
        self.last_ack = None;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CastTransition {
    Start,
    Stop,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ViewerCount {
    count: usize,
}

impl ViewerCount {
    pub fn count(&self) -> usize {
        self.count
    }

    pub fn join(&mut self) -> Option<CastTransition> {
        self.count += 1;
        (self.count == 1).then_some(CastTransition::Start)
    }

    pub fn leave(&mut self) -> Option<CastTransition> {
        if self.count == 0 {
            return None;
        }
        self.count -= 1;
        (self.count == 0).then_some(CastTransition::Stop)
    }
}

/// Frames a latest-wins viewer never saw between two deliveries.
pub fn skipped_between(last_seen: u64, delivered: u64) -> u64 {
    delivered.saturating_sub(last_seen).saturating_sub(1)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameMetadata {
    #[serde(default)]
    pub offset_top: f64,
    #[serde(default = "unit_scale")]
    pub page_scale_factor: f64,
    #[serde(default)]
    pub device_width: f64,
    #[serde(default)]
    pub device_height: f64,
    #[serde(default)]
    pub scroll_offset_x: f64,
    #[serde(default)]
    pub scroll_offset_y: f64,
    /// Float seconds since the epoch, not milliseconds.
    #[serde(default)]
    pub timestamp: Option<f64>,
}

fn unit_scale() -> f64 {
    1.0
}

impl FrameMetadata {
    pub fn still(viewport: Viewport, now_secs: f64) -> Self {
        Self {
            offset_top: 0.0,
            page_scale_factor: 1.0,
            device_width: viewport.width as f64,
            device_height: viewport.height as f64,
            scroll_offset_x: 0.0,
            scroll_offset_y: 0.0,
            timestamp: Some(now_secs),
        }
    }

    pub fn latency_ms(&self, now_secs: f64) -> Option<f64> {
        self.timestamp
            .map(|captured| ((now_secs - captured) * 1000.0).max(0.0))
    }
}

pub fn unix_now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .unwrap_or(0.0)
}

#[derive(Clone, Debug, PartialEq)]
pub struct RawFrame {
    pub ack_id: i64,
    pub data: String,
    pub metadata: FrameMetadata,
}

/// `params` of a `Page.screencastFrame` event.
pub fn parse_frame(params: &Value) -> Result<RawFrame, String> {
    let ack_id = params
        .get("sessionId")
        .and_then(Value::as_i64)
        .ok_or("screencast frame without an ack id")?;
    let data = params
        .get("data")
        .and_then(Value::as_str)
        .ok_or("screencast frame without data")?
        .to_string();
    let metadata = params
        .get("metadata")
        .cloned()
        .map(serde_json::from_value::<FrameMetadata>)
        .transpose()
        .map_err(|error| format!("screencast metadata: {error}"))?
        .ok_or("screencast frame without metadata")?;
    Ok(RawFrame {
        ack_id,
        data,
        metadata,
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreencastParams {
    pub viewport: Viewport,
    pub quality: u8,
}

impl ScreencastParams {
    pub fn new(viewport: Viewport) -> Self {
        Self {
            viewport,
            quality: DEFAULT_QUALITY,
        }
    }

    pub fn start_json(&self) -> Value {
        let (max_width, max_height) = self.viewport.device_size();
        json!({
            "format": "jpeg",
            "quality": self.quality.min(100),
            "maxWidth": max_width,
            "maxHeight": max_height,
            "everyNthFrame": 1,
        })
    }
}

#[derive(Debug, PartialEq)]
pub struct ScreencastFrame {
    pub seq: u64,
    pub jpeg: Vec<u8>,
    pub metadata: FrameMetadata,
    pub still: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HubNoticeKind {
    Plain,
    /// The window is occluded or minimised, so no still could be captured; clears with the next frame.
    PreviewPaused,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HubNotice {
    pub kind: HubNoticeKind,
    pub text: String,
}

#[derive(Debug)]
pub struct FrameDelivery {
    pub frame: Arc<ScreencastFrame>,
    pub skipped: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DriverCommand {
    Start(ScreencastParams),
    Restart(ScreencastParams),
    Stop,
}

struct HubState {
    viewers: ViewerCount,
    seq: u64,
    generation: u64,
    driver: Option<mpsc::UnboundedSender<DriverCommand>>,
    params: ScreencastParams,
    closed: bool,
    captures: usize,
}

/// A cast frame sized unlike the viewport is one Chrome re-rendered for a screenshot, not the page.
fn off_viewport(metadata: &FrameMetadata, viewport: Viewport) -> bool {
    let differs = |device: f64, css: u32| device > 0.0 && (device - css as f64).abs() > 1.0;
    differs(metadata.device_width, viewport.width)
        || differs(metadata.device_height, viewport.height)
}

/// While held, cast frames that do not match the viewport are dropped instead of shown.
pub struct CaptureHold {
    hub: Arc<FrameHub>,
}

impl Drop for CaptureHold {
    fn drop(&mut self) {
        let mut state = self.hub.state.lock();
        state.captures = state.captures.saturating_sub(1);
    }
}

/// Host-level frame source: survives browser relaunches, so viewers never resubscribe.
pub struct FrameHub {
    frames: watch::Sender<Option<Arc<ScreencastFrame>>>,
    notices: broadcast::Sender<HubNotice>,
    state: Mutex<HubState>,
}

impl FrameHub {
    pub fn new(params: ScreencastParams) -> Arc<Self> {
        Arc::new(Self {
            frames: watch::channel(None).0,
            notices: broadcast::channel(16).0,
            state: Mutex::new(HubState {
                viewers: ViewerCount::default(),
                seq: 0,
                generation: 0,
                driver: None,
                params,
                closed: false,
                captures: 0,
            }),
        })
    }

    pub fn subscribe(self: &Arc<Self>) -> ScreencastSubscription {
        let frames = self.frames.subscribe();
        let last_seq = {
            let mut state = self.state.lock();
            if state.viewers.join() == Some(CastTransition::Start) {
                let params = state.params;
                send_command(&state.driver, DriverCommand::Start(params));
            }
            state.seq
        };
        ScreencastSubscription {
            hub: self.clone(),
            frames,
            last_seq,
        }
    }

    fn leave(&self) {
        let mut state = self.state.lock();
        if state.viewers.leave() == Some(CastTransition::Stop) {
            send_command(&state.driver, DriverCommand::Stop);
        }
    }

    pub fn viewer_count(&self) -> usize {
        self.state.lock().viewers.count()
    }

    pub fn params(&self) -> ScreencastParams {
        self.state.lock().params
    }

    /// A new runtime's driver takes over; frames still in flight from the old one are dropped.
    pub fn bind(&self, driver: mpsc::UnboundedSender<DriverCommand>) -> u64 {
        let mut state = self.state.lock();
        state.generation += 1;
        if state.viewers.count() > 0 {
            let _ = driver.send(DriverCommand::Start(state.params));
        }
        state.driver = Some(driver);
        state.generation
    }

    pub fn unbind(&self, generation: u64) {
        let mut state = self.state.lock();
        if state.generation == generation {
            state.driver = None;
        }
    }

    pub fn set_params(&self, params: ScreencastParams) {
        let mut state = self.state.lock();
        if state.params == params {
            return;
        }
        state.params = params;
        if state.viewers.count() > 0 {
            send_command(&state.driver, DriverCommand::Restart(params));
        }
    }

    pub fn publish(
        &self,
        generation: u64,
        jpeg: Vec<u8>,
        metadata: FrameMetadata,
        still: bool,
    ) -> bool {
        let frame = {
            let mut state = self.state.lock();
            if state.closed || state.generation != generation {
                return false;
            }
            if state.captures > 0 && !still && off_viewport(&metadata, state.params.viewport) {
                return false;
            }
            state.seq += 1;
            Arc::new(ScreencastFrame {
                seq: state.seq,
                jpeg,
                metadata,
                still,
            })
        };
        self.frames.send_replace(Some(frame));
        true
    }

    pub fn latest(&self) -> Option<Arc<ScreencastFrame>> {
        self.frames.borrow().clone()
    }

    pub fn hold_for_capture(self: &Arc<Self>) -> CaptureHold {
        self.state.lock().captures += 1;
        CaptureHold { hub: self.clone() }
    }

    pub fn notice(&self, text: impl Into<String>) {
        self.send_notice(HubNoticeKind::Plain, text.into());
    }

    pub fn preview_paused(&self, detail: impl Into<String>) {
        self.send_notice(HubNoticeKind::PreviewPaused, detail.into());
    }

    fn send_notice(&self, kind: HubNoticeKind, text: String) {
        let _ = self.notices.send(HubNotice { kind, text });
    }

    pub fn notices(&self) -> broadcast::Receiver<HubNotice> {
        self.notices.subscribe()
    }

    pub fn close(&self) {
        {
            let mut state = self.state.lock();
            state.closed = true;
            state.driver = None;
        }
        self.frames.send_replace(None);
    }

    fn is_closed(&self) -> bool {
        self.state.lock().closed
    }
}

fn send_command(driver: &Option<mpsc::UnboundedSender<DriverCommand>>, command: DriverCommand) {
    if let Some(driver) = driver {
        let _ = driver.send(command);
    }
}

/// Latest-wins: a backlogged viewer gets the newest frame and a count of the ones it missed.
pub struct ScreencastSubscription {
    hub: Arc<FrameHub>,
    frames: watch::Receiver<Option<Arc<ScreencastFrame>>>,
    last_seq: u64,
}

impl ScreencastSubscription {
    /// `None` once the hub closed.
    pub async fn next(&mut self) -> Option<FrameDelivery> {
        loop {
            self.frames.changed().await.ok()?;
            let frame = self.frames.borrow_and_update().clone();
            match frame {
                Some(frame) => {
                    let skipped = skipped_between(self.last_seq, frame.seq);
                    self.last_seq = frame.seq;
                    return Some(FrameDelivery { frame, skipped });
                }
                None if self.hub.is_closed() => return None,
                None => continue,
            }
        }
    }

    pub fn latest(&self) -> Option<Arc<ScreencastFrame>> {
        self.hub.latest()
    }
}

impl Drop for ScreencastSubscription {
    fn drop(&mut self) {
        self.hub.leave();
    }
}

/// Owns the screencast of one runtime's page; dropping it hands the hub to the next runtime.
pub struct ScreencastDriver {
    hub: Arc<FrameHub>,
    generation: u64,
}

impl ScreencastDriver {
    pub fn spawn(page: PageChannel, hub: Arc<FrameHub>) -> Self {
        let (commands, receiver) = mpsc::unbounded_channel();
        let generation = hub.bind(commands);
        tokio::spawn(run_driver(page, hub.clone(), generation, receiver));
        Self { hub, generation }
    }
}

impl Drop for ScreencastDriver {
    fn drop(&mut self) {
        self.hub.unbind(self.generation);
    }
}

async fn run_driver(
    page: PageChannel,
    hub: Arc<FrameHub>,
    generation: u64,
    mut commands: mpsc::UnboundedReceiver<DriverCommand>,
) {
    let mut frames = page.subscribe_session(&["Page.screencastFrame"]);
    let mut pacer = AckPacer::new(ACK_INTERVAL);
    let mut casting = false;
    let mut pending_ack: Option<(i64, tokio::time::Instant)> = None;
    loop {
        let ack_at = pending_ack
            .map(|(_, at)| at)
            .unwrap_or_else(tokio::time::Instant::now);
        tokio::select! {
            command = commands.recv() => {
                let Some(command) = command else { break };
                match command {
                    DriverCommand::Start(params) if !casting => {
                        casting = start_cast(&page, &hub, generation, params).await;
                    }
                    DriverCommand::Start(_) => {}
                    DriverCommand::Restart(params) => {
                        if casting {
                            stop_cast(&page).await;
                        }
                        pending_ack = None;
                        pacer.reset();
                        casting = start_cast(&page, &hub, generation, params).await;
                    }
                    DriverCommand::Stop => {
                        if casting {
                            stop_cast(&page).await;
                        }
                        casting = false;
                        pending_ack = None;
                        pacer.reset();
                    }
                }
            }
            event = frames.recv() => {
                let Some(event) = event else { break };
                let raw = match parse_frame(&event.params) {
                    Ok(raw) => raw,
                    Err(error) => {
                        log::warn!("[browser-screencast] {error}");
                        continue;
                    }
                };
                if !casting {
                    continue;
                }
                match base64::engine::general_purpose::STANDARD.decode(raw.data.as_bytes()) {
                    Ok(jpeg) => {
                        hub.publish(generation, jpeg, raw.metadata, false);
                    }
                    Err(error) => log::warn!("[browser-screencast] frame is not base64: {error}"),
                }
                let due = pacer.ack_due(Instant::now());
                pending_ack = Some((raw.ack_id, tokio::time::Instant::from_std(due)));
            }
            // A rate cap, not a readiness wait: the ack is owed either way and time only delays it.
            _ = tokio::time::sleep_until(ack_at), if pending_ack.is_some() => {
                if let Some((ack_id, _)) = pending_ack.take() {
                    if let Err(error) = page.send("Page.screencastFrameAck", json!({ "sessionId": ack_id })).await {
                        log::warn!("[browser-screencast] ack failed: {error}");
                    }
                    pacer.acked(Instant::now());
                }
            }
        }
    }
}

async fn start_cast(
    page: &PageChannel,
    hub: &FrameHub,
    generation: u64,
    params: ScreencastParams,
) -> bool {
    let _ = page.send("Page.bringToFront", json!({})).await;
    if let Err(error) = page.send("Page.startScreencast", params.start_json()).await {
        hub.notice(format!("screencast could not start: {error}"));
        return false;
    }
    push_still(page, hub, generation, params).await;
    true
}

async fn stop_cast(page: &PageChannel) {
    if let Err(error) = page.send("Page.stopScreencast", json!({})).await {
        log::warn!("[browser-screencast] stop failed: {error}");
    }
}

/// An idle page never repaints, so a fresh viewer gets one still; an occluded app may never render.
async fn push_still(page: &PageChannel, hub: &FrameHub, generation: u64, params: ScreencastParams) {
    let still = page
        .send_with_deadline(
            "Page.captureScreenshot",
            json!({ "format": "jpeg", "quality": params.quality.min(100) }),
            STILL_DEADLINE,
        )
        .await;
    let data = match still {
        Ok(result) => result
            .get("data")
            .and_then(Value::as_str)
            .map(str::to_string),
        Err(error) => {
            hub.preview_paused(format!("no frame: window not rendering ({error})"));
            return;
        }
    };
    let Some(jpeg) = data.and_then(|data| {
        base64::engine::general_purpose::STANDARD
            .decode(data.as_bytes())
            .ok()
    }) else {
        hub.preview_paused("no frame: the still capture returned no image");
        return;
    };
    hub.publish(
        generation,
        jpeg,
        FrameMetadata::still(params.viewport, unix_now_secs()),
        true,
    );
}

#[cfg(test)]
mod tests;
