import type { CSSProperties } from "react";
import { boundedNotice } from "./boundedNotice";

interface Props {
  text: string;
  className?: string;
  summaryClassName?: string;
  summaryStyle?: CSSProperties;
}

/** A status or error line kept to one bounded sentence; the raw text stays under Details. */
export function BoundedNoticeText({ text, className = "", summaryClassName, summaryStyle }: Props) {
  const notice = boundedNotice(text);
  return (
    <div className={`flex min-w-0 flex-col gap-1 ${className}`}>
      <span className={summaryClassName} style={summaryStyle}>{notice.summary}</span>
      {notice.details && (
        <details className="w-full min-w-0 text-left">
          <summary className="cursor-pointer select-none">Details</summary>
          <pre className="mt-1 max-h-64 overflow-auto whitespace-pre-wrap break-words font-mono text-[10px]">{notice.details}</pre>
        </details>
      )}
    </div>
  );
}
