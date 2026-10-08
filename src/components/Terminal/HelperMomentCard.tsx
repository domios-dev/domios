import { useEffect, useMemo } from "react";
import { Lightbulb } from "lucide-react";
import { parseChatBlock } from "@codeterm/chat-ui";
import { ensurePluginSnapshot } from "../../lib/pluginSnapshotStore";
import { blockSource, blocksInText } from "../../lib/helperMoments/helperBlocks";
import { suggestionKey } from "../../lib/helperMoments/helperRules";
import type { HelperMomentsStore } from "../../lib/helperMoments/helperMomentsStore";
import { helperMoments, useHelperSuggestion } from "../../lib/helperMoments/helperMomentsWiring";

const LINK_BUTTON = "rounded px-1.5 py-0.5 text-xs text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]";

/** The host's own helper line in the General Agent chat: one benefit sentence and one live block, shown only while the agent is idle. */
export function HelperMomentCard({ idle, latestAnswer, store = helperMoments }: { idle: boolean; latestAnswer: string | null; store?: HelperMomentsStore }) {
  const suggestion = useHelperSuggestion(store);
  const source = suggestion ? blockSource(suggestion.block) : null;
  const answered = useMemo(() => new Set(blocksInText(latestAnswer ?? "").map(blockSource)), [latestAnswer]);
  const duplicate = source !== null && answered.has(source);
  const visible = idle && suggestion !== null && !duplicate;
  const key = suggestion ? suggestionKey(suggestion) : null;

  useEffect(() => {
    void ensurePluginSnapshot({ background: true }).catch(() => {});
  }, []);
  useEffect(() => {
    if (idle && latestAnswer) store.offerAdjacent(latestAnswer);
  }, [idle, latestAnswer, store]);
  useEffect(() => {
    if (idle && duplicate) store.discard();
  }, [idle, duplicate, store]);
  useEffect(() => {
    if (visible && key) store.markShown();
  }, [visible, key, store]);

  const block = useMemo(() => (source ? parseChatBlock(source) : null), [source]);
  if (!visible || !suggestion || !block) return null;
  return (
    <aside
      aria-label="Suggestion from Domios"
      data-testid="helper-moment"
      data-helper-moment={suggestion.momentId}
      className="my-3 border-l-2 pl-3"
      style={{ borderColor: "color-mix(in srgb, var(--ct-accent) 55%, transparent)" }}
    >
      <p className="m-0 mb-2 flex items-start gap-2 text-[13px] leading-5 text-[var(--ct-fg)]">
        <Lightbulb size={14} aria-hidden="true" className="mt-[3px] shrink-0 text-[var(--ct-accent)]" />
        <span>{suggestion.benefit}</span>
      </p>
      <div
        onClickCapture={(event) => {
          if ((event.target as Element).closest?.("[data-block-action]")) store.accept();
        }}
      >
        {block.render()}
      </div>
      <div className="mt-1 flex items-center gap-1">
        <button type="button" className={LINK_BUTTON} onClick={() => store.dismiss()}>
          {suggestion.accepted ? "Close" : "Not now"}
        </button>
        {!suggestion.accepted && (
          <button type="button" className={LINK_BUTTON} onClick={() => store.mute()}>
            Don't show again
          </button>
        )}
      </div>
    </aside>
  );
}
