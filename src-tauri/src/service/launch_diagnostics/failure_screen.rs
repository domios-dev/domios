use crate::service::ServiceContext;

const MAX_LINES: usize = 4;
const MAX_LINE_CHARS: usize = 512;

fn screen_tail(screen: &str) -> Vec<String> {
    let mut lines: Vec<_> = screen
        .lines()
        .rev()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .take(MAX_LINES)
        .map(|line| {
            line.chars()
                .filter(|character| !character.is_control())
                .take(MAX_LINE_CHARS)
                .collect()
        })
        .collect();
    lines.reverse();
    lines
}

pub(crate) fn log_failure_screen(svc: &ServiceContext, tab_id: &str, pty_id: &str, reason: &str) {
    let screen = { svc.screen_state.lock().get_screen_text(pty_id) };
    let tail = screen.as_deref().map(screen_tail).unwrap_or_default();
    log::warn!(
        "[launch-failure-output] tab={tab_id} pty={pty_id} reason={reason} screen_tail={}",
        serde_json::to_string(&tail).unwrap_or_default()
    );
}

#[cfg(test)]
mod tests;
