use std::ffi::{OsStr, OsString};
use std::sync::{Mutex, MutexGuard};

static PROCESS_ENV: Mutex<()> = Mutex::new(());

pub struct EnvGuard {
    _lock: MutexGuard<'static, ()>,
    previous: Vec<(OsString, Option<OsString>)>,
}

impl EnvGuard {
    pub fn lock() -> Self {
        Self {
            _lock: PROCESS_ENV
                .lock()
                .unwrap_or_else(|error| error.into_inner()),
            previous: Vec::new(),
        }
    }

    pub fn set(&mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) {
        let key = key.as_ref().to_os_string();
        self.remember(&key);
        std::env::set_var(key, value);
    }

    pub fn remove(&mut self, key: impl AsRef<OsStr>) {
        let key = key.as_ref().to_os_string();
        self.remember(&key);
        std::env::remove_var(key);
    }

    fn remember(&mut self, key: &OsString) {
        if self.previous.iter().any(|(saved, _)| saved == key) {
            return;
        }
        self.previous.push((key.clone(), std::env::var_os(key)));
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.previous.drain(..).rev() {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}
