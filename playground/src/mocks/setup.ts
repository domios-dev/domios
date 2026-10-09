// Product modules read this at import time to choose the desktop transport.
(window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__ = {};
