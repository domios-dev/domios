import { createRoot, type Root } from "react-dom/client";
import { MessageBubble } from "../MessageBubble";
import { CURIE_PARTIAL_REPORT } from "./inboundReportFixtures.testdata";

declare global {
  interface Window {
    __codetermInboundReportHarness?: {
      mount: (host: HTMLElement) => Root;
    };
  }
}

export function mountInboundReportCardHarness(host: HTMLElement): Root {
  const root = createRoot(host);
  root.render(
    <div style={{ minWidth: 0, maxWidth: "100%" }}>
      <div data-testid="report-variant-local">
        <MessageBubble
          message={{
            id: "harness-report-local",
            type: "user",
            content: CURIE_PARTIAL_REPORT,
            fromLabel: "Curie",
            fromPane: "02d1339c",
            inboundKind: "report",
          }}
        />
      </div>
      <div data-testid="report-variant-mesh">
        <MessageBubble
          message={{
            id: "harness-report-mesh",
            type: "user",
            content: CURIE_PARTIAL_REPORT,
            fromLabel: "Curie",
            fromPane: "02d1339c",
            fromMesh: "blackstar",
            inboundKind: "report",
          }}
        />
      </div>
    </div>,
  );
  return root;
}

export function installInboundReportCardHarness(testMode: boolean): void {
  if (!testMode || typeof window === "undefined" || window.__codetermInboundReportHarness) return;
  window.__codetermInboundReportHarness = { mount: mountInboundReportCardHarness };
}
