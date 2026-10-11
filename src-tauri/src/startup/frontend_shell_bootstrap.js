(() => {
  if (window.top !== window) return;
  const bootstrap = __CODETERM_SHELL_BOOTSTRAP__;
  const fallbackOrigins = ["tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"];
  if (window.location.origin !== bootstrap.origin && !fallbackOrigins.includes(window.location.origin)) return;
  window.__CODETERM_DAEMON__ = bootstrap;
})();
