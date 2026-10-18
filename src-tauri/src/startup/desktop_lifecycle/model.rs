#[derive(Default, Debug, PartialEq, Eq)]
pub(super) enum Frontend {
    #[default]
    Absent,
    Creating,
    Open,
    Closing {
        reopen: bool,
    },
    ShuttingDown,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Activation {
    Create,
    Raise,
    Wait,
}

impl Frontend {
    pub(super) fn activate(&mut self) -> Activation {
        match self {
            Self::Absent => {
                *self = Self::Creating;
                Activation::Create
            }
            Self::Open => Activation::Raise,
            Self::Closing { reopen } => {
                *reopen = true;
                Activation::Wait
            }
            Self::Creating | Self::ShuttingDown => Activation::Wait,
        }
    }

    pub(super) fn created(&mut self, success: bool) {
        if *self == Self::Creating {
            *self = if success { Self::Open } else { Self::Absent };
        }
    }

    pub(super) fn close(&mut self) -> bool {
        if *self != Self::Open {
            return false;
        }
        *self = Self::Closing { reopen: false };
        true
    }

    pub(super) fn destroyed(&mut self) -> bool {
        if *self == Self::ShuttingDown {
            return false;
        }
        let reopen = *self == Self::Closing { reopen: true };
        *self = Self::Absent;
        reopen
    }
}

pub(super) fn retain_daemon(exit_code: Option<i32>, shutting_down: bool) -> bool {
    exit_code.is_none() && !shutting_down
}

pub(crate) fn single_instance_enabled(debug: bool, identifier: &str) -> bool {
    !debug || identifier != "com.codeterm.app"
}

#[cfg(test)]
mod tests;
