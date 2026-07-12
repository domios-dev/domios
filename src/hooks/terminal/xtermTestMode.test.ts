import { shouldEnableXtermA11yMode } from "./xtermTestMode";

function assertEqual(actual: boolean, expected: boolean, label: string) {
  if (actual !== expected) {
    throw new Error(`${label}: expected ${expected}, got ${actual}`);
  }
}

assertEqual(
  shouldEnableXtermA11yMode({
    isViteDev: true,
    viteCodeTermTest: "1",
    locationSearch: "",
  }),
  true,
  "dev harness enables a11y terminal mode",
);

assertEqual(
  shouldEnableXtermA11yMode({
    isViteDev: false,
    viteCodeTermTest: "1",
    locationSearch: "",
  }),
  false,
  "release ignores leaked VITE_CODETERM_TEST",
);

assertEqual(
  shouldEnableXtermA11yMode({
    isViteDev: false,
    viteCodeTermTest: undefined,
    locationSearch: "?a11y=1",
  }),
  false,
  "release ignores runtime a11y query",
);

assertEqual(
  shouldEnableXtermA11yMode({
    isViteDev: false,
    viteCodeTermTest: undefined,
    locationSearch: "?e2e=1",
    locationProtocol: "http:",
    locationHostname: "127.0.0.1",
    locationPort: "1420",
  }),
  true,
  "local dev runtime e2e query enables terminal DOM mirror",
);

assertEqual(
  shouldEnableXtermA11yMode({
    isViteDev: false,
    viteCodeTermTest: undefined,
    locationSearch: "?e2e=1",
    locationProtocol: "http:",
    locationHostname: "127.0.0.1",
    locationPort: "7685",
  }),
  false,
  "prod port ignores runtime e2e query",
);

assertEqual(
  shouldEnableXtermA11yMode({
    isViteDev: true,
    viteCodeTermTest: undefined,
    locationSearch: "",
  }),
  false,
  "normal dev keeps WebGL terminal mode",
);

console.log("xterm test-mode gate checks passed");
