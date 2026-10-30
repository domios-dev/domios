import { useRef, useState } from "react";
import { ComposerMentionPicker } from "../../../../src/components/Terminal/ComposerMentionPicker";
import { useComposerMentions } from "../../../../src/components/Terminal/useComposerMentions";
import { Button } from "../../../../src/components/shared/Button";
import type { ComposerMention, MentionScope, createMentionRegistry } from "../../../../src/lib/composerMentions";
import "../../../../src/components/Terminal/composerMentionPicker.css";
import { defineSpecimen } from "../types";

const scope: MentionScope = { tabId: "composer", workspaceId: "gallery", projectPath: "/project", teamId: "team" };
const rows: ComposerMention[] = [
  { id: "rivet", kind: "tab", providerId: "claude", label: "Rivet", description: "Worker · Builds the release and checks a deliberately long description for overflow", group: "Tabs in this Team", insertText: "@Rivet (tab rivet)", match: 1 },
  { id: "river", kind: "tab", providerId: "codex", label: "River", description: "Worker · Reviews scoped suggestions", group: "Tabs in this Team", insertText: "@River (tab river)", match: 1 },
  { id: "shell", kind: "tab", label: "Shell", description: "Shell · Project terminal", group: "Tabs in this Team", insertText: "@Shell (tab shell)", match: 1 },
  { id: "task", kind: "task", label: "COD-2173", status: "in_progress", description: "Stable composer mention picker", group: "Tasks", insertText: "COD-2173", match: 1 },
  { id: "plan", kind: "plan", label: "Release", description: "Active · Interface improvements", group: "Plans", insertText: "@Release (plan release)", match: 1 },
  { id: "blueprint", kind: "blueprint", label: "Review", description: "Waiting · Review incoming changes", group: "Blueprints", insertText: "@Review (blueprint review)", match: 1 },
];
function MentionsSpecimen({ mode }: { mode: "ready" | "initial" | "empty" | "error" }) {
  const [draft, setDraft] = useState("@");
  const [pending, setPending] = useState(0);
  const textarea = useRef<HTMLTextAreaElement>(null);
  const anchor = useRef<HTMLDivElement>(null);
  const requests = useRef<{ resolve: (rows: ComposerMention[]) => void; reject: (error: Error) => void; query: string }[]>([]);
  const [registry] = useState<ReturnType<typeof createMentionRegistry>>(() => ({
    search(query) {
      if (!query && mode !== "initial") return mode === "error" ? Promise.reject(new Error("fixture")) : Promise.resolve(mode === "empty" ? [] : rows);
      return new Promise((resolve, reject) => {
        requests.current.push({ resolve, reject, query });
        setPending(requests.current.length);
      });
    },
    subscribe: () => () => {}, close: () => {},
  }));
  const picker = useComposerMentions(draft, setDraft, () => textarea.current?.focus(), textarea, registry, scope);
  const settle = (outcome: "rows" | "empty" | "error", oldest = false) => {
    const request = oldest ? requests.current.shift() : requests.current.pop();
    if (!request) return;
    if (outcome === "error") request.reject(new Error("fixture"));
    else request.resolve(outcome === "empty" ? [] : rows.filter((row) => row.label.toLowerCase().includes(request.query.toLowerCase())));
    setPending(requests.current.length);
    textarea.current?.focus();
  };
  return <div style={{ minHeight: 420, display: "flex", flexDirection: "column", justifyContent: "space-between", gap: 16 }}>
    <div data-anchored-popover="" onPointerDown={(event) => event.preventDefault()} style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
      <Button onClick={() => settle("rows")} disabled={!pending}>Resolve newest</Button>
      <Button onClick={() => settle("rows", true)} disabled={!pending}>Resolve oldest</Button>
      <Button onClick={() => settle("empty")} disabled={!pending}>No matches</Button>
      <Button onClick={() => settle("error")} disabled={!pending}>Fail newest</Button>
      <span aria-live="polite">{pending} queries held</span>
    </div>
    <div ref={anchor} style={{ width: "100%" }}>
      <label>Message <textarea ref={textarea} value={draft} style={{ display: "block", width: "100%" }}
        onChange={(event) => { setDraft(event.currentTarget.value); picker.syncSelection(event.currentTarget); }}
        onSelect={(event) => picker.syncSelection(event.currentTarget)} onKeyDown={picker.handleKey} /></label>
      <ComposerMentionPicker key={picker.session} {...picker} onHighlight={picker.setHighlighted} onPick={picker.pick} onClose={picker.close} triggerRef={anchor} />
    </div>
  </div>;
}
function InteractiveMentions() {
  const [mode, setMode] = useState<"ready" | "initial" | "empty" | "error">("ready");
  return <>
    <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
      {(["ready", "initial", "empty", "error"] as const).map((state) => <Button key={state} onClick={() => setMode(state)} aria-pressed={mode === state}>{state}</Button>)}
    </div>
    <MentionsSpecimen key={mode} mode={mode} />
  </>;
}
export default defineSpecimen({
  id: "composer-mentions", title: "Composer mentions", category: "Composer", stage: "wide",
  where: "src/components/Terminal/ComposerMentionPicker.tsx",
  notes: "Real hook and menu. Focus the textarea; type @r, @ri, @riv, @rive, @rivet while queries are held. Resolve oldest to exercise obsolete completion, then newest. Check arrows, Home/End, Page keys, Enter/Tab, Escape, outside and touch. Review at 1280 and 380 in both themes.",
  Component: InteractiveMentions,
  states: [{ id: "interactive", label: "Initial, refresh, empty and failure controls", props: {} }],
});
