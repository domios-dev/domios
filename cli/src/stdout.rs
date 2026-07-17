use std::any::Any;
use std::panic::{self, AssertUnwindSafe, PanicHookInfo};

pub fn run(main_impl: impl FnOnce()) {
    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        if !is_broken_pipe_panic(info) {
            previous_hook(info);
        }
    }));

    match panic::catch_unwind(AssertUnwindSafe(main_impl)) {
        Ok(()) => {}
        Err(payload) if is_broken_pipe_payload(payload.as_ref()) => std::process::exit(0),
        Err(payload) => panic::resume_unwind(payload),
    }
}

fn is_broken_pipe_panic(info: &PanicHookInfo<'_>) -> bool {
    is_broken_pipe_payload(info.payload())
}

fn is_broken_pipe_payload(payload: &(dyn Any + Send)) -> bool {
    payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .is_some_and(is_broken_pipe_message)
}

fn is_broken_pipe_message(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("broken pipe")
        || message.contains("epipe")
        || message.contains("os error 32")
        || message.contains("os error 109")
        || message.contains("os error 232")
}

#[cfg(test)]
mod tests {
    use super::is_broken_pipe_message;

    #[test]
    fn recognizes_windows_and_unix_broken_pipe_messages() {
        for message in [
            "failed printing to stdout: Broken pipe (os error 32)",
            "stdout write failed: The pipe has been ended. (os error 109)",
            "stdout write failed: The pipe is being closed. (os error 232)",
            "stdout failed: EPIPE",
        ] {
            assert!(is_broken_pipe_message(message), "{message}");
        }
    }

    #[test]
    fn preserves_unrelated_panics() {
        assert!(!is_broken_pipe_message("index out of bounds"));
    }
}
