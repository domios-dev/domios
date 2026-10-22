use super::{ComposerInputState, PromptSignal, PtyScreen, ScreenChangeSignal, ShellReadySignal};
use codeterm_util::lock_hold::TimedMutex;
use std::collections::HashMap;
use std::ops::DerefMut;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

type ScreenMap = Arc<TimedMutex<HashMap<String, Arc<ScreenEntry>>>>;

struct ScreenEntry {
    state: TimedMutex<PtyScreen>,
    removed: AtomicBool,
    handles: AtomicUsize,
}

pub struct ScreenHandle {
    screens: ScreenMap,
    pty_id: String,
    entry: Arc<ScreenEntry>,
}

impl ScreenHandle {
    pub fn lock(&self) -> impl DerefMut<Target = PtyScreen> + '_ {
        self.entry.state.lock()
    }

    pub fn lock_for_update(&self) -> impl DerefMut<Target = PtyScreen> + '_ {
        let state = self.entry.state.lock();
        self.entry.removed.store(false, Ordering::SeqCst);
        state
    }

    pub fn try_lock(&self) -> Option<impl DerefMut<Target = PtyScreen> + '_> {
        self.entry.state.try_lock()
    }
}

impl Drop for ScreenHandle {
    fn drop(&mut self) {
        if self.entry.handles.fetch_sub(1, Ordering::AcqRel) != 1
            || !self.entry.removed.load(Ordering::SeqCst)
        {
            return;
        }
        let mut screens = self.screens.lock();
        // Release the pin before checking retirement so concurrent drops have exactly one last owner.
        if self.entry.handles.load(Ordering::Acquire) == 0
            && self.entry.removed.load(Ordering::SeqCst)
            && screens
                .get(&self.pty_id)
                .is_some_and(|current| Arc::ptr_eq(current, &self.entry))
        {
            screens.remove(&self.pty_id);
        }
    }
}

pub struct ScreenState {
    screens: ScreenMap,
}

impl Default for ScreenState {
    fn default() -> Self {
        Self::new()
    }
}

impl ScreenState {
    pub fn new() -> Self {
        Self {
            screens: Arc::new(TimedMutex::new("screen_state_map", HashMap::new())),
        }
    }

    fn handle(&self, pty_id: &str, entry: Arc<ScreenEntry>) -> ScreenHandle {
        entry.handles.fetch_add(1, Ordering::Relaxed);
        ScreenHandle {
            screens: self.screens.clone(),
            pty_id: pty_id.to_string(),
            entry,
        }
    }

    pub fn existing(&self, pty_id: &str) -> Option<ScreenHandle> {
        let screens = self.screens.lock();
        let entry = screens.get(pty_id)?;
        if entry.removed.load(Ordering::SeqCst) {
            return None;
        }
        Some(self.handle(pty_id, entry.clone()))
    }

    pub fn contains_pty(&self, pty_id: &str) -> bool {
        self.screens.lock().contains_key(pty_id)
    }

    pub fn pty(&self, pty_id: &str) -> ScreenHandle {
        let mut screens = self.screens.lock();
        let entry = screens
            .entry(pty_id.to_string())
            .or_insert_with(|| {
                Arc::new(ScreenEntry {
                    state: TimedMutex::new("screen_state_parser", PtyScreen::default()),
                    removed: AtomicBool::new(false),
                    handles: AtomicUsize::new(0),
                })
            })
            .clone();
        self.handle(pty_id, entry)
    }

    fn with_pty<T>(&self, pty_id: &str, read: impl FnOnce(&mut PtyScreen) -> T) -> T {
        let screen = self.pty(pty_id);
        let mut screen = screen.lock_for_update();
        read(&mut screen)
    }

    fn with_existing<T>(&self, pty_id: &str, read: impl FnOnce(&mut PtyScreen) -> T) -> Option<T> {
        let screen = self.existing(pty_id)?;
        let mut state = screen.entry.state.lock();
        Some(read(&mut state))
    }

    pub fn process(&self, pty_id: &str, data: &[u8]) {
        self.with_pty(pty_id, |screen| screen.process(data))
    }

    pub fn process_if_generation(&self, pty_id: &str, generation: u64, data: &[u8]) -> bool {
        self.with_existing(pty_id, |screen| {
            screen.process_if_generation(generation, data)
        })
        .unwrap_or_default()
    }

    pub fn resize(&self, pty_id: &str, cols: u16, rows: u16) {
        self.with_pty(pty_id, |screen| screen.resize(cols, rows))
    }

    pub fn get_screen_text(&self, pty_id: &str) -> Option<String> {
        self.with_existing(pty_id, |screen| screen.get_screen_text())
            .flatten()
    }

    pub fn get_screen_rows(&self, pty_id: &str) -> Option<Vec<String>> {
        self.with_existing(pty_id, |screen| screen.get_screen_rows())
            .flatten()
    }

    pub fn win32_input_mode(&self, pty_id: &str) -> bool {
        self.with_existing(pty_id, |screen| screen.win32_input_mode())
            .unwrap_or_default()
    }

    pub fn bracketed_paste_mode(&self, pty_id: &str) -> Option<bool> {
        self.with_existing(pty_id, |screen| screen.bracketed_paste_mode())
            .flatten()
    }

    pub fn reset_input_modes(&self, pty_id: &str) -> bool {
        self.with_existing(pty_id, |screen| screen.reset_input_modes())
            .unwrap_or_default()
    }

    pub fn native_prompt_input(
        &self,
        pty_id: &str,
    ) -> Option<crate::native_prompt::NativePromptInput> {
        self.with_existing(pty_id, |screen| screen.native_prompt_input())
            .flatten()
    }

    pub fn cursor_input_state(&self, pty_id: &str) -> Option<(bool, bool, u16, u16)> {
        self.with_existing(pty_id, |screen| screen.cursor_input_state())
            .flatten()
    }

    pub fn composer_input_state(&self, pty_id: &str) -> Option<ComposerInputState> {
        self.with_existing(pty_id, |screen| screen.composer_input_state())
            .flatten()
    }

    pub fn get_screen_formatted(&self, pty_id: &str) -> Option<Vec<u8>> {
        self.with_existing(pty_id, |screen| screen.get_screen_formatted())
            .flatten()
    }

    pub fn get_content_hash(&self, pty_id: &str, exclude_bottom_rows: u16) -> Option<[u8; 32]> {
        self.with_existing(pty_id, |screen| {
            screen.get_content_hash(exclude_bottom_rows)
        })
        .flatten()
    }

    pub fn lines_and_tail(&self, pty_id: &str, tail_rows: usize) -> Option<(Vec<String>, String)> {
        self.with_existing(pty_id, |screen| screen.lines_and_tail(tail_rows))
            .flatten()
    }

    pub fn tail_text(&self, pty_id: &str, max_rows: usize) -> Option<String> {
        self.with_existing(pty_id, |screen| screen.tail_text(max_rows))
            .flatten()
    }

    pub fn arm_prompt_signal(&self, pty_id: &str) -> Arc<PromptSignal> {
        self.with_pty(pty_id, |screen| screen.arm_prompt_signal())
    }

    pub fn arm_bare_prompt_signal(&self, pty_id: &str) -> Arc<PromptSignal> {
        self.with_pty(pty_id, |screen| screen.arm_bare_prompt_signal())
    }

    pub fn arm_shell_ready_signal(&self, pty_id: &str) -> Arc<ShellReadySignal> {
        self.with_pty(pty_id, |screen| screen.arm_shell_ready_signal())
    }

    pub fn has_pty_generation(&self, pty_id: &str) -> bool {
        self.with_existing(pty_id, |screen| screen.has_pty_generation())
            .unwrap_or_default()
    }

    pub fn disarm_bare_prompt_signal(&self, pty_id: &str, signal: &Arc<PromptSignal>) {
        let _ = self.with_existing(pty_id, |screen| screen.disarm_bare_prompt_signal(signal));
    }

    pub fn arm_agent_signal(&self, pty_id: &str) -> Arc<PromptSignal> {
        self.with_pty(pty_id, |screen| screen.arm_agent_signal())
    }

    pub fn pty_generation(&self, pty_id: &str) -> Option<u64> {
        self.with_existing(pty_id, |screen| screen.pty_generation())
            .flatten()
    }

    pub fn arm_screen_change_signal(&self, pty_id: &str) -> Arc<ScreenChangeSignal> {
        self.with_pty(pty_id, |screen| screen.arm_screen_change_signal())
    }

    pub fn bind_pty_generation(&self, pty_id: &str, generation: u64) -> bool {
        self.with_pty(pty_id, |screen| screen.bind_pty_generation(generation))
    }

    pub fn subscribe_screen_changes(&self, pty_id: &str) -> Arc<ScreenChangeSignal> {
        self.with_pty(pty_id, |screen| screen.subscribe_screen_changes())
    }

    pub fn unsubscribe_screen_changes(&self, pty_id: &str, signal: &Arc<ScreenChangeSignal>) {
        let _ = self.with_existing(pty_id, |screen| screen.unsubscribe_screen_changes(signal));
        signal.close();
    }

    pub fn screen_change_subscriber_count(&self) -> usize {
        let screens: Vec<_> = self
            .screens
            .lock()
            .iter()
            .map(|(pty_id, entry)| self.handle(pty_id, entry.clone()))
            .collect();
        screens
            .iter()
            .map(|screen| screen.entry.state.lock().screen_change_subscribers.len())
            .sum()
    }

    #[cfg(test)]
    pub(super) fn bare_prompt_waiter_count(&self, pty_id: &str) -> usize {
        self.with_existing(pty_id, |screen| screen.bare_prompt_signals.len())
            .unwrap_or_default()
    }

    pub fn remove(&self, pty_id: &str) {
        if let Some(screen) = self.existing(pty_id) {
            let mut state = screen.entry.state.lock();
            state.remove();
            screen.entry.removed.store(true, Ordering::SeqCst);
        }
    }

    pub fn remove_if_generation(&self, pty_id: &str, generation: u64) -> bool {
        let Some(screen) = self.existing(pty_id) else {
            return false;
        };
        let mut state = screen.entry.state.lock();
        if state.pty_generation() != Some(generation) {
            return false;
        }
        state.remove();
        screen.entry.removed.store(true, Ordering::SeqCst);
        true
    }
}

#[cfg(test)]
mod tests;
