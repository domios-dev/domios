# Chat button concept

## Shipped rule

Use a round button for one compact, icon-only action at a stable square footprint; use a rectangle for text actions and a pill for compact grouped/status content. Radio controls are circular because they express a single choice, while multi-select controls are square. The circle is an action target, not a general-purpose radius. [ComposerAttachButton.tsx:14-28](../src/components/Terminal/ComposerAttachButton.tsx#L14-L28), [InteractiveQuestion.tsx:91-115](../packages/chat-ui/src/interactive/InteractiveQuestion.tsx#L91-L115).

## Composer placement and states

The composer is a three-part row: 40px attach circle, flexible rounded input shell, 40px send/voice circle. The schedule control is the exceptional 20px circle *inside* the input shell, visible only when text changes the primary action to Send. [ComposerActions.tsx:74-89](../src/components/Terminal/ComposerActions.tsx#L74-L89), [ComposerInputShell.tsx:34-76](../src/components/Terminal/ComposerInputShell.tsx#L34-L76).

| State | Shipped behavior | Reference |
| --- | --- | --- |
| Rest / hover | Attach is surface + border and grows to 1.03 on hover; the idle main action is the same 40px surface circle with a mic. | [ComposerAttachButton.tsx:20-27](../src/components/Terminal/ComposerAttachButton.tsx#L20-L27), [ComposerActionButton.tsx:43-97](../src/components/Terminal/ComposerActionButton.tsx#L43-L97) |
| Send | Text changes the main circle to accent-backed Send; it is disabled when sending is unavailable. | [useComposerVoice.ts:217-224](../src/components/Terminal/voice/useComposerVoice.ts#L217-L224), [ComposerActionButton.tsx:137-160](../src/components/Terminal/ComposerActionButton.tsx#L137-L160) |
| Upload / disabled | Attach disables and dims to 40% while uploading; main action does the same while voice is busy or Send is disabled. | [ComposerAttachButton.tsx:14-27](../src/components/Terminal/ComposerAttachButton.tsx#L14-L27), [ComposerActionButton.tsx:85-90](../src/components/Terminal/ComposerActionButton.tsx#L85-L90) |
| Hold recording | Holding the mic replaces the input with recording UI, expands the action bubble to 88px, and follows the pointer with elastic stretch. | [useComposerVoice.ts:85-110](../src/components/Terminal/voice/useComposerVoice.ts#L85-L110), [composerBubble.tsx:30-44](../src/components/Terminal/composerBubble.tsx#L30-L44), [ComposerActionButton.tsx:154-158](../src/components/Terminal/ComposerActionButton.tsx#L154-L158) |
| Locked / cancel | Dragging up locks recording and changes the icon to a dark stop square with an accent ring; sliding into cancel arms a red X. | [ComposerActionButton.tsx:102-134](../src/components/Terminal/ComposerActionButton.tsx#L102-L134), [useComposerVoice.ts:150-154](../src/components/Terminal/voice/useComposerVoice.ts#L150-L154) |
| Preview | Recording becomes an in-shell preview; discard, play, and send are separately round icon controls. | [ComposerInputShell.tsx:44-62](../src/components/Terminal/ComposerInputShell.tsx#L44-L62), [VoicePreview.tsx:65-119](../src/components/Terminal/voice/VoicePreview.tsx#L65-L119) |

Playground motion is deliberately additive: centered hover swell and centered press/release only, with no glow. It must not imply the production recording drag physics where a static specimen cannot provide that interaction.

<!-- revision: 1 -->
