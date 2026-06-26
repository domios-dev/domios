import type { ReactNode } from "react";
import { Check, Scale } from "lucide-react";
import { RichMarkdown } from "@codeterm/chat-ui";
import type { CanvasElement } from "../types";
import { atLeast, World } from "../worlds";
import type { CanvasCardStyleProps } from "./types";
import { digestMarkdown } from "./digest";
import { Eyebrow, bodyRegionStyle, clampStyle, firstString, provenanceCardStyle } from "./provenanceShell";

// §26 decision artifact — a recorded ruling, firmer than a note. Renders its
// STRUCTURED fields as a ladder (rationale → options → chosen → verdict) rather
// than flattening to markdown, so the reasoning reads at a glance. Far bands show
// the ruling itself (chosen + verdict) so the card is meaningful; the deep dive
// reveals every field untruncated via node-internal scroll.
const DECISION_ACCENT = "var(--ct-accent, rgba(99,102,241,0.92))";
const VERDICT_GREEN = "rgba(82,180,145,0.97)";
const VERDICT_RED = "rgba(248,113,113,0.97)";
const VERDICT_AMBER = "rgba(245,158,11,0.97)";

interface DecisionParts {
  title: string;
  rationale: string;
  options: string[];
  chosen: string;
  verdict: string;
  body: string;
}

function asStringList(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value
    .map((it) => {
      if (typeof it === "string") return it;
      if (it && typeof it === "object") return firstString(it as Record<string, unknown>, ["label", "text", "name", "option", "title"]);
      return "";
    })
    .map((s) => s.trim())
    .filter(Boolean);
}

function parseDecision(element: CanvasElement): DecisionParts {
  if (typeof element.content === "string") {
    return { title: "", rationale: "", options: [], chosen: "", verdict: "", body: element.content };
  }
  if (!element.content || typeof element.content !== "object") {
    return { title: "", rationale: "", options: [], chosen: "", verdict: "", body: "" };
  }
  const c = element.content as Record<string, unknown>;
  return {
    title: firstString(c, ["title", "question", "topic"]),
    rationale: firstString(c, ["rationale", "why", "reason", "reasoning"]),
    options: asStringList(c.options ?? c.alternatives ?? c.candidates),
    chosen: firstString(c, ["chosen", "choice", "decision", "selected", "pick"]),
    verdict: firstString(c, ["verdict", "outcome", "resolution", "status"]),
    body: firstString(c, ["body", "markdown", "text", "detail", "details"]),
  };
}

function verdictColor(verdict: string): string {
  const v = verdict.toLowerCase();
  if (/accept|approv|adopt|done|ship|resolv|final|yes/.test(v)) return VERDICT_GREEN;
  if (/reject|declin|drop|no\b|abandon|revert/.test(v)) return VERDICT_RED;
  if (/pend|open|defer|review|tbd|propos/.test(v)) return VERDICT_AMBER;
  return DECISION_ACCENT;
}

function VerdictChip({ verdict }: { verdict: string }) {
  const color = verdictColor(verdict);
  return (
    <span
      data-testid="canvas-decision-verdict"
      style={{
        flexShrink: 0,
        fontSize: "0.64em",
        fontWeight: 800,
        letterSpacing: "0.04em",
        textTransform: "uppercase",
        padding: "0.1em 0.5em",
        borderRadius: 999,
        color,
        background: `color-mix(in srgb, ${color} 14%, transparent)`,
        border: `1px solid color-mix(in srgb, ${color} 50%, transparent)`,
        whiteSpace: "nowrap",
        maxWidth: 120,
        overflow: "hidden",
        textOverflow: "ellipsis",
      }}
    >
      {verdict}
    </span>
  );
}

function Field({ label, children, testid }: { label: string; children: ReactNode; testid?: string }) {
  return (
    <div data-testid={testid} style={{ display: "flex", flexDirection: "column", gap: 3, minWidth: 0 }}>
      <span style={{ fontSize: "0.6em", fontWeight: 800, letterSpacing: "0.1em", textTransform: "uppercase", opacity: 0.6 }}>{label}</span>
      {children}
    </div>
  );
}

function ChosenRow({ chosen }: { chosen: string }) {
  return (
    <div
      data-testid="canvas-decision-chosen"
      style={{
        display: "flex",
        alignItems: "center",
        gap: 6,
        minWidth: 0,
        padding: "0.2em 0.55em",
        borderRadius: "var(--ct-radius-sm)",
        background: `color-mix(in srgb, ${VERDICT_GREEN} 13%, transparent)`,
        border: `1px solid color-mix(in srgb, ${VERDICT_GREEN} 45%, transparent)`,
      }}
    >
      <Check size={12} strokeWidth={2.6} style={{ flexShrink: 0, color: VERDICT_GREEN }} aria-hidden />
      <span data-canvas-selectable className="ct-canvas-text" style={{ fontSize: "0.86em", fontWeight: 700, minWidth: 0, overflowWrap: "anywhere" }}>{chosen}</span>
    </div>
  );
}

export interface DecisionCardProps extends CanvasCardStyleProps {
  element: CanvasElement;
  world: World;
}

export function DecisionCard({ element, style, world }: DecisionCardProps) {
  const { title, rationale, options, chosen, verdict, body } = parseDecision(element);
  const deep = atLeast(world, World.W3);
  const showStructure = atLeast(world, World.W2);
  const showVerdict = atLeast(world, World.W1) && !!verdict;
  const titleClamp = deep ? null : atLeast(world, World.W1) ? 3 : 2;
  const hasStructured = !!(rationale || options.length || chosen);
  const hasBody = body.trim().length > 0;
  const headline = title || chosen || digestMarkdown(body, 200);
  const chosenLc = chosen.trim().toLowerCase();

  return (
    <div
      data-testid="canvas-decision-card"
      data-element-id={element.id}
      data-world={world}
      className="ct-canvas-node"
      style={{ ...provenanceCardStyle(DECISION_ACCENT), padding: "9px 11px", gap: 6, ...style }}
    >
      <Eyebrow
        icon={<Scale size={12} strokeWidth={2.2} />}
        label="Decision"
        accent={DECISION_ACCENT}
        testid="canvas-decision-label"
        trailing={showVerdict ? <VerdictChip verdict={verdict} /> : undefined}
      />

      {headline && (title || !showStructure) && (
        <div
          data-testid="canvas-decision-headline"
          data-canvas-selectable
          className="ct-canvas-text"
          style={{ flexShrink: 0, fontSize: "0.98em", fontWeight: 700, lineHeight: 1.25, letterSpacing: "-0.01em", overflowWrap: "break-word", ...clampStyle(titleClamp) }}
        >
          {title || headline}
        </div>
      )}

      {/* Far-band ruling glance: when there's no room for the full ladder, the chosen
          pill stands in so the card still says what was decided (never куцо). */}
      {!showStructure && chosen && <ChosenRow chosen={chosen} />}

      <div className="ct-canvas-scroll" data-canvas-scroll-body={deep ? "1" : undefined} style={bodyRegionStyle(world)}>
        {showStructure && (
          <>
            {rationale && (
              <Field label="Rationale" testid="canvas-decision-rationale">
                <span data-canvas-selectable className="ct-canvas-text" style={{ fontSize: "0.85em", lineHeight: 1.45, overflowWrap: "anywhere", ...(deep ? {} : clampStyle(4)) }}>{rationale}</span>
              </Field>
            )}
            {options.length > 0 && (
              <Field label="Options" testid="canvas-decision-options">
                <div style={{ display: "flex", flexWrap: "wrap", gap: 5 }}>
                  {options.map((opt, i) => {
                    const isChosen = chosenLc.length > 0 && opt.trim().toLowerCase() === chosenLc;
                    const color = isChosen ? VERDICT_GREEN : "var(--ct-border-subtle)";
                    return (
                      <span
                        key={i}
                        data-canvas-selectable
                        className="ct-canvas-text"
                        title={opt}
                        style={{
                          display: "inline-flex",
                          alignItems: "center",
                          gap: 4,
                          fontSize: "0.78em",
                          fontWeight: isChosen ? 700 : 500,
                          padding: "0.1em 0.5em",
                          borderRadius: 999,
                          maxWidth: deep ? "100%" : 160,
                          background: isChosen ? `color-mix(in srgb, ${VERDICT_GREEN} 13%, transparent)` : "var(--ct-bg-active)",
                          border: `1px solid ${isChosen ? `color-mix(in srgb, ${VERDICT_GREEN} 45%, transparent)` : color}`,
                          color: "var(--ct-fg)",
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                        }}
                      >
                        {isChosen && <Check size={10} strokeWidth={2.8} style={{ flexShrink: 0, color: VERDICT_GREEN }} aria-hidden />}
                        {opt}
                      </span>
                    );
                  })}
                </div>
              </Field>
            )}
            {chosen && !options.some((o) => o.trim().toLowerCase() === chosenLc) && (
              <Field label="Chosen" testid="canvas-decision-chosen-field">
                <ChosenRow chosen={chosen} />
              </Field>
            )}
            {hasBody && (
              <div
                data-testid="canvas-decision-body"
                data-canvas-selectable
                className="ct-canvas-text"
                style={{ fontSize: "0.86em", lineHeight: 1.5, minHeight: 0, overflowWrap: "anywhere", ...(hasStructured ? { borderTop: "1px solid var(--ct-border-subtle)", paddingTop: 6 } : {}), ...(deep ? {} : clampStyle(5)) }}
              >
                {deep ? <RichMarkdown text={body} /> : digestMarkdown(body, 320)}
              </div>
            )}
          </>
        )}
      </div>
    </div>
  );
}
