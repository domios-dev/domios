import type React from "react";
import { Bug, CalendarClock, Database, Hammer, Rocket, ScrollText, Server, SquareTerminal, Workflow, type LucideIcon } from "lucide-react";
import type { ChatOrigin } from "@codeterm/chat-core";

type AutomationOrigin = Extract<ChatOrigin, { kind: "automation" }>;

const UTILITY: Record<string, { icon: LucideIcon; label: string }> = {
  build: { icon: Hammer, label: "Build" },
  dev: { icon: Server, label: "Server" },
  qa: { icon: Bug, label: "QA" },
  deploy: { icon: Rocket, label: "Deploy" },
  logs: { icon: ScrollText, label: "Logs" },
  db: { icon: Database, label: "Database" },
};

export function automationLook(origin: AutomationOrigin): { icon: LucideIcon; kind: string; tone: string } {
  if (origin.source === "cron") return { icon: CalendarClock, kind: "Cron", tone: "var(--ct-tone-cron)" };
  if (origin.source === "blueprint") return { icon: Workflow, kind: "Blueprint", tone: "var(--ct-tone-blueprint)" };
  const purpose = origin.purpose ?? "shell";
  const look = UTILITY[purpose];
  return { icon: look?.icon ?? SquareTerminal, kind: look?.label ?? "Shell", tone: `var(--ct-tone-${look ? purpose : "shell"})` };
}

function detailLabel(origin: AutomationOrigin): string | null {
  if (origin.report) return "report";
  if (!origin.detail) return null;
  return origin.detail.replace(/[:_]/g, " ").toLowerCase();
}

/** A cron, blueprint, or utility-shell delivery as its own colour-coded card. */
export function AutomationCard({ origin, onOpen, openLabel, children }: {
  origin: AutomationOrigin;
  onOpen?: () => void;
  openLabel?: string;
  children: React.ReactNode;
}): React.ReactElement {
  const { icon: Icon, kind, tone } = automationLook(origin);
  const detail = detailLabel(origin);
  return (
    <div className="mb-3 w-full max-w-4xl min-w-0 mx-auto" data-testid="chat-automation-card" data-source={origin.source} data-purpose={origin.purpose}>
      <div className="ct-card" style={{ "--ct-card-tone": tone } as React.CSSProperties}>
        <div className="ct-card__head">
          <span className="ct-card__icon"><Icon size={14} strokeWidth={2} aria-hidden="true" /></span>
          <span className="min-w-0 flex-1">
            <span className="ct-card__title">
              {kind}
              <span className="ct-card__meta">· {origin.name}</span>
              {detail && <span className="ct-card__meta">· {detail}</span>}
            </span>
          </span>
          {onOpen && (
            <button type="button" className="ct-bar-pill" style={{ "--ct-tab-pill-h": "24px" } as React.CSSProperties} onClick={onOpen}>
              {openLabel ?? "Open"}
            </button>
          )}
        </div>
        <div className="ct-card__body">{children}</div>
      </div>
    </div>
  );
}
