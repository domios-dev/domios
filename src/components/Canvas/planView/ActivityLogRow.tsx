import { useState, type CSSProperties, type ReactNode } from "react";
import { ChevronRight } from "lucide-react";

export interface ActivityLogRowProps {
  kind: string;
  color?: string;
  age: string;
  stamp?: string;
  stampTitle?: string;
  author?: string | null;
  lead?: string;
  text: string;
  onOpen?: () => void;
  children?: ReactNode;
  testIds?: { row?: string; kind?: string; age?: string; stamp?: string; author?: string };
  data?: Record<string, string | undefined>;
}

/** One activity event as a calm log row; expands to the full text unless it opens something. */
export function ActivityLogRow({ kind, color, age, stamp, stampTitle, author, lead, text, onOpen, children, testIds = {}, data = {} }: ActivityLogRowProps) {
  const [expanded, setExpanded] = useState(false);
  const flat = text.replace(/\s+/g, " ").trim();
  return (
    <div
      className="ct-row ct-activity-row"
      data-expanded={expanded || undefined}
      data-testid={testIds.row}
      style={{ "--ct-activity-tone": color ?? "var(--ct-muted)" } as CSSProperties}
      onClick={onOpen}
      {...Object.fromEntries(Object.entries(data).map(([k, v]) => [`data-${k}`, v]))}
    >
      <button type="button" className="ct-activity-row__head" aria-expanded={onOpen ? undefined : expanded} onClick={onOpen ? undefined : () => setExpanded((v) => !v)}>
        <ChevronRight size={12} aria-hidden="true" className="ct-activity-row__chevron" />
        <span className="ct-activity-row__age" data-testid={testIds.age} title={stampTitle}>{age}</span>
        <span className="ct-activity-row__kind" data-testid={testIds.kind}><span className="ct-activity-row__dot" aria-hidden="true" />{kind}</span>
        <span className="ct-activity-row__who">
          {lead && <span className="ct-activity-row__lead">{lead}</span>}
          {author && <span data-testid={testIds.author}>{author}</span>}
        </span>
        <span className="ct-activity-row__text">{flat}</span>
      </button>
      <div className="ct-activity-row__detail" hidden={!expanded}>
        {stamp && <div className="ct-activity-row__stamp" data-testid={testIds.stamp} title={stampTitle}>{stamp}</div>}
        <p className="ct-activity-row__full">{text}</p>
        {children}
      </div>
    </div>
  );
}
