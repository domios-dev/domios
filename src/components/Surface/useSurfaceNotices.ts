import { useEffect } from "react";
import { publishNotice } from "../../lib/notices";
import { authoredCause } from "../../lib/describeError";
import type { SurfaceNotice } from "./surfaceStreamTransport";

export function useSurfaceNotices(notice: SurfaceNotice | null, needsHuman: string | null, surfaceId: string) {
  useEffect(() => {
    if (!notice) return;
    publishNotice({ kind: "toast", severity: notice.tone === "error" ? "Error" : notice.tone === "warn" ? "Warning" : "Info",
      id: `surface-notice:${surfaceId}`, title: "Browser notice", detail: notice.tone === "error" ? authoredCause(notice.text) : notice.text,
    });
  }, [notice, surfaceId]);
  useEffect(() => {
    if (!needsHuman) return;
    publishNotice({ kind: "toast", severity: "Warning", id: `surface-needs-human:${surfaceId}`, title: "Browser needs your attention", detail: needsHuman });
  }, [needsHuman, surfaceId]);
}
