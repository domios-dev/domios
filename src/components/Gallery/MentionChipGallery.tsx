import { useRef, useState, type CSSProperties } from "react";
import { ReferenceChip } from "../../../packages/chat-ui/src/codeterm/ReferenceChip";
import { MessageBubble } from "../../../packages/chat-ui/src/MessageBubble";
import { AgentIcon } from "../AgentIcon";
import { ComposerTextInput } from "../Terminal/ComposerTextInput";
import { ComposerMentionHighlight } from "../Terminal/ComposerMentionHighlight";
import { useComposerMentionTokens } from "../Terminal/useComposerMentionTokens";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { DEFAULT_THEME, themeToCSS } from "../../lib/themeCss";

const TAB_MARKER = '<domios type="tab" id="gallery-elgar" />';
const mux = createMuxStateStore(() => ({ tabs: {
  "gallery-elgar": { id: "gallery-elgar", title: "Elgar", tab_type: "Terminal", pty_id: null, cwd: null },
}, workspaces: [] } as unknown as ExtendedState));
const light = { ...DEFAULT_THEME, name: "github-light", background: "#f6f8fa", foreground: "#1f2328", blue: "#0969da", bright_black: "#57606a" };

function MentionEditor() {
  const [canonical, setCanonical] = useState("");
  const editor = useRef<HTMLTextAreaElement>(null);
  const mentions = useComposerMentionTokens(canonical, setCanonical, editor);
  return <>
    <button type="button" onClick={() => {
      const el = editor.current!;
      mentions.insert({ id: "elgar", label: "Elgar", kind: "tab", insertText: TAB_MARKER, description: "", group: "Tabs", match: 0 },
        { start: el.selectionStart, end: el.selectionEnd, partial: "" });
    }}>Insert Elgar</button>
    <p>Select part of a token, use the arrow and delete keys, then undo. Paste a raw tab marker or copy/cut this token.</p>
    <div style={{ position: "relative", padding: 0, fontSize: 14, lineHeight: "24px", marginTop: 12 }}>
      <ComposerMentionHighlight draft={mentions.draft} textarea={editor} marginPx={0} />
      <ComposerTextInput ref={editor} aria-label="Mention review composer" placeholder="Message" value={mentions.draft.text}
        style={{ fontSize: 14, lineHeight: "24px", height: 96 }}
        onChange={event => mentions.change(event.currentTarget.value, (event.nativeEvent as InputEvent).inputType)}
        onBeforeInput={mentions.beforeInput} onCompositionStart={() => { if (editor.current) mentions.snapSelection(editor.current); }}
        onKeyDown={event => { if (!event.nativeEvent.isComposing) mentions.handleKey(event); }}
        onSelect={event => mentions.snapSelection(event.currentTarget)}
        onPaste={mentions.handlePaste} onCopy={event => mentions.handleCopy(event, false)} onCut={event => mentions.handleCopy(event, true)} />
    </div>
    <span role="status" aria-live="polite">{mentions.announcement}</span>
    <details><summary>Canonical draft</summary><pre style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{canonical}</pre></details>
  </>;
}

function ChipSet() {
  return <>
    <ReferenceChip kind="tab" presentation="mention" label="Elgar" fullRef="gallery-elgar" disabled={false}
      participant={{ tabKind: "agent", providerName: "Claude", role: "Orchestrator", workspaceName: "codeterm", activity: "working", avatar: <AgentIcon providerId={null} style={{ width: 12, height: 12 }} /> }} />{" "}
    <ReferenceChip kind="tab" presentation="mention" label="Iris" fullRef="gallery-iris" disabled={false} participant={{ tabKind: "agent", activity: "waiting" }} />{" "}
    <ReferenceChip kind="tab" presentation="mention" label="Resting" fullRef="gallery-idle" disabled={false} participant={{ tabKind: "agent", activity: "idle" }} />{" "}
    <ReferenceChip kind="tab" presentation="mention" label="Failed" fullRef="gallery-error" disabled={false} participant={{ tabKind: "agent", activity: "error" }} />{" "}
    <ReferenceChip kind="tab" presentation="mention" label="build" fullRef="gallery-shell" disabled={false} participant={{ tabKind: "shell" }} />{" "}
    <ReferenceChip kind="tab" presentation="mention" label="Browser" fullRef="gallery-browser" disabled={false} participant={{ tabKind: "browser" }} />{" "}
    <ReferenceChip kind="tab" presentation="mention" label="Elgar" fullRef="gallery-loading" disabled state="loading" />{" "}
    <ReferenceChip kind="tab" presentation="mention" label="Closed" fullRef="gallery-closed" disabled state="closed" participant={{ tabKind: "agent" }} />{" "}
    <ReferenceChip kind="tab" presentation="mention" label="Missing" fullRef="gallery-missing" disabled state="missing" />{" "}
    <ReferenceChip kind="tab" presentation="mention" label="A tab title that is longer than twenty four characters" fullRef="gallery-long" disabled={false} participant={{ tabKind: "agent" }} />
  </>;
}

export function MentionChipGallery() {
  const [theme, setTheme] = useState<"dark" | "light">("dark");
  return <div data-testid="mention-review" style={{ ...themeToCSS(theme === "light" ? light : DEFAULT_THEME) as CSSProperties,
    background: "var(--ct-bg)", color: "var(--ct-fg)", padding: 16, borderRadius: 16, maxWidth: 720 }}>
    <div style={{ display: "flex", gap: 16 }}><button type="button" onClick={() => setTheme("dark")}>Dark</button><button type="button" onClick={() => setTheme("light")}>Light</button></div>
    <p>Tabs only. People are deferred for this release.</p>
    <div style={{ lineHeight: "30px", margin: "16px 0" }}><ChipSet /></div>
    <div className="ct-user-bubble"><ChipSet /></div>
    <div style={{ maxWidth: 340, marginTop: 20 }}>
      <MessageBubble message={{ id: "gallery-files", type: "user", content: 'Review <domios type="attachment" kind="file" path="/project/config.json" /> alongside <domios type="attachment" kind="file" path="/project/typography.md" /> and <domios type="attachment" kind="file" path="/project/pkg-typography-notes.md" /> before sending the update.' }} />
      <MessageBubble message={{ id: "gallery-relayed-files", type: "user", content: 'Review **these files**: <domios type="attachment" kind="file" path="/project/config.json" /> alongside <domios type="attachment" kind="file" path="/project/typography.md" /> and <domios type="attachment" kind="file" path="/project/pkg-typography-notes.md" /> before sending the update.' }} origin={{ kind: "tab", title: "Elgar", tabIdShort: "gallery-elgar" }} />
    </div>
    <div style={{ marginTop: 20 }}><MuxStateStoreContext value={mux}><MentionEditor /></MuxStateStoreContext></div>
  </div>;
}
