use crate::headless::{Error, ErrorCode};

pub fn marker_refusal(marker_present: bool) -> Option<Error> {
    marker_present.then(|| {
        Error::new(
            ErrorCode::ContainerManagedUpdate,
            codeterm_paths::container_install::UPDATE_MESSAGE,
        )
    })
}

pub fn refusal() -> Option<Error> {
    marker_refusal(codeterm_paths::container_install::marked())
}

#[cfg(test)]
mod tests;
