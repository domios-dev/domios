// Copyright 2019-2024 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

mod windows;

pub fn error<S: AsRef<str>>(err: S) {
  windows::error(err);
}
