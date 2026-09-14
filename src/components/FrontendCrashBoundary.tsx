import { Component, type ErrorInfo, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { clearStateSnapshot } from "../lib/stateCache";
import { clearRelayTarget } from "../lib/webApi";

export const SAFE_START_KEY = "codeterm:crash-safe-start";

/** Same document, relay reset to this machine — a hash-only diff so `replace` + `reload` is one deterministic load. */
function safeStartUrl(location: { origin: string; pathname: string; search: string }): string {
  return `${location.origin}${location.pathname}${location.search}#relay=self`;
}

function armSafeStart(): void {
  try {
    window.sessionStorage.setItem(SAFE_START_KEY, "1");
  } catch {
  }
}

function consumeSafeStart(): boolean {
  try {
    if (!window.sessionStorage.getItem(SAFE_START_KEY)) return false;
    window.sessionStorage.removeItem(SAFE_START_KEY);
    return true;
  } catch {
    return false;
  }
}

function hardNavigate(url: string): void {
  window.location.replace(url);
  window.location.reload();
}

type Props = {
  children: ReactNode;
  /** Injected by tests; production performs a real document navigation. */
  navigate?: (url: string) => void;
};
type State = { error: Error | null; safeStart: boolean };

const FALLBACK_ERROR_MESSAGE = "The interface failed without an error message.";
const FALLBACK_STACK = "No stack trace was available.";

function crashDetails(error: Error): { message: string; stack: string } {
  const rawMessage = error.message.trim();
  const message = rawMessage || FALLBACK_ERROR_MESSAGE;
  const stack = error.stack?.trimEnd() ?? "";
  if (!stack.trim()) return { message, stack: FALLBACK_STACK };

  const lines = stack.split(/\r?\n/);
  const firstLine = lines[0]?.trim() ?? "";
  if (rawMessage && !firstLine.startsWith("at ") && !firstLine.startsWith("at\t") && firstLine.endsWith(rawMessage)) {
    lines.shift();
  }

  const remainingStack = lines.join("\n").trimEnd();
  return { message, stack: remainingStack.trim() ? remainingStack : FALLBACK_STACK };
}

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
  state: State = { error: null, safeStart: consumeSafeStart() };

  static getDerivedStateFromError(error: Error): Pick<State, "error"> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error("[frontend-crash] React render failed", error, info.componentStack);
  }

  // The snapshot cache is keyed by relay target, so it must be dropped before the target is.
  private leaveCrashedLocation = (): void => {
    clearStateSnapshot();
    clearRelayTarget();
    armSafeStart();
    (this.props.navigate ?? hardNavigate)(safeStartUrl(window.location));
  };

  private openAfterSafeStart = (): void => {
    this.setState({ error: null, safeStart: false });
  };

  render(): ReactNode {
    if (this.state.safeStart) {
      return (
        <div className="frontend-crash__shell">
          <CrashWindowChrome />
          <main className="frontend-crash" role="status" aria-labelledby="frontend-safe-start-title">
            <section className="frontend-crash__panel">
              <p className="frontend-crash__eyebrow">Safe start</p>
              <h1 id="frontend-safe-start-title">Left the location that crashed</h1>
              <p>
                The restored workspace snapshot and the remote machine target were cleared, so CodeTerm opens on this
                machine instead of re-entering the view that failed.
              </p>
              <div className="frontend-crash__actions">
                <button type="button" onClick={this.openAfterSafeStart}>
                  Open CodeTerm
                </button>
              </div>
            </section>
          </main>
        </div>
      );
    }

    if (!this.state.error) return this.props.children;

    const details = crashDetails(this.state.error);

    return (
      <div className="frontend-crash__shell">
        <CrashWindowChrome />
        <main className="frontend-crash" role="alert" aria-labelledby="frontend-crash-title">
          <section className="frontend-crash__panel">
            <p className="frontend-crash__eyebrow">CodeTerm UI error</p>
            <h1 id="frontend-crash-title">The interface could not render</h1>
            <p>The daemon and terminals may still be running. Reload the interface after reviewing the error.</p>
            <section className="frontend-crash__detail" aria-labelledby="frontend-crash-message-label">
              <h2 id="frontend-crash-message-label">Error message</h2>
              <p className="frontend-crash__message" id="frontend-crash-message" data-testid="frontend-crash-message">
                {details.message}
              </p>
            </section>
            <section className="frontend-crash__detail" aria-labelledby="frontend-crash-stack-label">
              <h2 id="frontend-crash-stack-label">Stack trace</h2>
              <pre className="frontend-crash__stack" id="frontend-crash-stack" data-testid="frontend-crash-stack">
                {details.stack}
              </pre>
            </section>
            <div className="frontend-crash__actions">
              <button type="button" onClick={this.leaveCrashedLocation}>
                Leave this location
              </button>
              <button type="button" className="frontend-crash__secondary" onClick={() => window.location.reload()}>
                Reload interface
              </button>
            </div>
          </section>
        </main>
      </div>
    );
  }
}
