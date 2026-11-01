import "./composerMentionPicker.css";
import { useImperativeHandle, type Ref, type RefObject } from "react";
import { ComposerMentionPicker } from "./ComposerMentionPicker";
import { useComposerMentions } from "./useComposerMentions";
import { useComposerMentionSources } from "./useComposerMentionSources";
import { useComposerMentionAnchor } from "./useComposerMentionAnchor";

export type ComposerMentionsHandle = Pick<ReturnType<typeof useComposerMentions>, "handleKey" | "syncSelection">;

export function ComposerMentions({ tabId, input, setInput, onPicked, textareaRef, anchorRef, ref }: {
  tabId: string;
  input: string;
  setInput: (text: string) => void;
  onPicked: () => void;
  textareaRef: RefObject<HTMLTextAreaElement | null>;
  anchorRef: RefObject<HTMLElement | null>;
  ref: Ref<ComposerMentionsHandle>;
}) {
  const sources = useComposerMentionSources(tabId);
  const picker = useComposerMentions(input, setInput, onPicked, textareaRef, sources.registry, sources.scope);
  const anchorPoint = useComposerMentionAnchor(textareaRef, picker.active, picker.queryStart, picker.session);
  useImperativeHandle(ref, () => ({ handleKey: picker.handleKey, syncSelection: picker.syncSelection }));
  return <ComposerMentionPicker key={picker.session} {...picker} onHighlight={picker.setHighlighted}
    onPick={picker.pick} onClose={picker.close} triggerRef={anchorRef} anchorPoint={anchorPoint} />;
}
