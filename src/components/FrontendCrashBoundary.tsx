import { Component, type ErrorInfo, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

type Props = { children: ReactNode };
type State = { error: Error | null };

function CrashWindowChrome() {
  const appWindow = "__TAURI_INTERNALS__" in window ? getCurrentWindow() : null;
  return (
    <header className="frontend-crash__chrome" data-tauri-drag-region>
      <span data-tauri-drag-region>CodeTerm</span>
      {appWindow && (
        <nav aria-label="Window controls">
          <button type="button" aria-label="Minimize" onClick={() => void appWindow.minimize()}>—</button>
          <button type="button" aria-label="Maximize" onClick={() => void appWindow.toggleMaximize()}>□</button>
          <button type="button" aria-label="Close" onClick={() => void appWindow.close()}>×</button>
        </nav>
      )}
    </header>
  );
}

export class FrontendCrashBoundary extends Component<Props, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error("[frontend-crash] React render failed", error, info.componentStack);
  }

  render(): ReactNode {
    if (!this.state.error) return this.props.children;

    return (
      <div className="frontend-crash__shell">
        <CrashWindowChrome />
        <main className="frontend-crash" role="alert" aria-labelledby="frontend-crash-title">
          <section className="frontend-crash__panel">
            <p className="frontend-crash__eyebrow">CodeTerm UI error</p>
            <h1 id="frontend-crash-title">The interface could not render</h1>
            <p>The daemon and terminals may still be running. Reload the interface after reviewing the error.</p>
            <pre>{this.state.error.stack ?? this.state.error.message}</pre>
            <button type="button" onClick={() => window.location.reload()}>
              Reload interface
            </button>
          </section>
        </main>
      </div>
    );
  }
}
