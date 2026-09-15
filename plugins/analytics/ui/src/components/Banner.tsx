import type { ReactNode } from "react";
import { CircleAlert, TriangleAlert } from "lucide-react";

export function Banner({
  tone,
  title,
  children,
  action,
  testId,
}: {
  tone: "warn" | "error";
  title: string;
  children: ReactNode;
  action?: ReactNode;
  testId: string;
}) {
  const Icon = tone === "error" ? CircleAlert : TriangleAlert;
  return (
    <div data-testid={testId} role={tone === "error" ? "alert" : "status"} className="analytics-banner" data-tone={tone}>
      <Icon size={14} aria-hidden />
      <div>
        <p className="analytics-banner-title">{title}</p>
        <p className="mc-board-note">{children}</p>
      </div>
      {action}
    </div>
  );
}
