(() => {
  if (window.top !== window) return;
  const bootstrap = __CODETERM_SHELL_BOOTSTRAP__;
  const fallbackOrigins = ["tauri://localhost", "http://tauri.localhost", "https://tauri.localhost"];
  const fallback = fallbackOrigins.some(origin => window.location.href === origin + "/index.html");
  if (window.location.origin !== (bootstrap.frontendOrigin ?? bootstrap.origin) && !fallback) return;
  if (__CODETERM_SHELL_DEBUG__) {
    const diagnostic = (stage, failure) => {
      const name = failure?.name;
      const errorName = ["Error", "ReferenceError", "TypeError", "SyntaxError", "SecurityError", "FrontendUpdateError"].includes(name) ? name : undefined;
      const reason = ["invalid-client", "checkpoint-unavailable", "status-unavailable", "api-incompatible", "storage-unavailable", "boot-context-invalid", "unexpected"].includes(failure?.reason) ? failure.reason : undefined;
      const httpStatus = Number.isInteger(failure?.httpStatus) && failure.httpStatus >= 100 && failure.httpStatus <= 599 ? failure.httpStatus : undefined;
      const source = String(failure?.filename ?? "").split("/").pop().split("?")[0].split("#")[0];
      void fetch(bootstrap.origin + "/api/frontend/diagnostic", {
        method: "POST", headers: { "Content-Type": "application/json", "Authorization": "Bearer " + bootstrap.token },
        body: JSON.stringify({ stage, errorName, reason, httpStatus, source: /^[a-zA-Z0-9_.-]{1,100}$/.test(source) ? source : undefined,
          line: failure?.lineno, column: failure?.colno,
          native: "__TAURI_INTERNALS__" in window, injected: !!window.__CODETERM_DAEMON__, mounted: document.documentElement?.dataset.codetermMounted === "true" }),
      }).catch(() => {});
    };
    window.__CODETERM_SHELL_DIAGNOSTIC__ = diagnostic;
    window.addEventListener("error", event => diagnostic("script-error", { name: event.error?.name, filename: event.filename, lineno: event.lineno, colno: event.colno }));
    window.addEventListener("unhandledrejection", event => diagnostic("promise-error", { name: event.reason?.name }));
  }
  window.__CODETERM_DAEMON__ = bootstrap;
  window.__CODETERM_SHELL_DIAGNOSTIC__?.("bootstrap-injected");
})();
