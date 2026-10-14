import { useState } from "react";
import { Check, ChevronDown } from "lucide-react";
import { SentMessageBubble, pickUiFontScaleValue, sentInvocation } from "@codeterm/chat-ui";
import type { TabNameResolver } from "@codeterm/chat-ui";
import type { Notification } from "../../lib/types";
import { acceptsTap, chatAskChoiceState, isChoiceEnabled } from "./chatAskChoiceModel";
import {
  askAccessibleName,
  choiceAccessibleName,
  levelLabel,
  notificationLevel,
  notificationTone,
} from "../../lib/notifyLevels";
import { levelIcon } from "./levelIcon";

interface Props {
  notification: Notification;
  onAnswer: (id: string, label: string) => void;
  resolveTabName?: TabNameResolver;
  fontScale?: number;
}

/**
 * One agent question, inside the transcript.
 *
 * The container, the colour formula and the scale are the context-request and
 * system-prompt cards'. The answer is read from the notification; only the tap
 * itself is held here, so the choice shows at once and the record wins as soon
 * as it moves. This card and the bell panel stay two views of one record.
 */
export function ChatAskCard({ notification, onAnswer, resolveTabName, fontScale }: Props) {
  const [expanded, setExpanded] = useState(true);
  const [tapped, setTapped] = useState<string | null>(null);
  const level = notificationLevel(notification);
  const tone = notificationTone(notification);
  const LevelIcon = levelIcon(level);
  const name = askAccessibleName(level, notification.title);
  const heading = level === null ? notification.title : levelLabel(level);
  const bodyText = pickUiFontScaleValue(fontScale, ["text-xs", "text-sm", "text-base", "text-lg"]);
  const slot = notification.answer;
  const recorded = slot?.answered ?? null;
  const state = chatAskChoiceState(notification, tapped);
  const askedFrom = slot?.asked_from_tab ?? null;
  const choices = (notification.actions ?? []).filter((action) =>
    action.type === "answer" || action.type === "respond_prompt_option",
  );
  const edge = { borderColor: `color-mix(in srgb, ${tone} 18%, transparent)` };
  const choose = (label: string) => {
    if (!acceptsTap(state, label, recorded)) return;
    setTapped(label);
    onAnswer(notification.id, label);
  };

  return (
    <>
      <div className="mb-3 w-full max-w-4xl min-w-0 mx-auto">
        <div
          data-testid="chat-ask-card"
          data-notification-id={notification.id}
          data-notification-level={level ?? ""}
          role="group"
          aria-label={name}
          className="max-w-full min-w-0 rounded-md border"
          style={{
            background: `color-mix(in srgb, var(--ct-surface) 92%, ${tone} 8%)`,
            borderColor: `color-mix(in srgb, ${tone} 28%, transparent)`,
            color: "var(--ct-fg)",
          }}
        >
          <button
            type="button"
            aria-expanded={expanded}
            aria-label={`${expanded ? "Hide" : "Show"} message — ${name}`}
            onClick={() => setExpanded(!expanded)}
            className="flex w-full min-w-0 items-center gap-2 px-3 py-2 text-left transition-colors hover:brightness-110"
            style={{ color: "var(--ct-fg)" }}
          >
            <LevelIcon size={14} className="shrink-0" style={{ color: tone }} />
            <span className="min-w-0 flex-1">
              <span className="block truncate text-xs font-medium">{heading}</span>
              {level !== null && (
                <span className="block truncate text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
                  {notification.title}
                </span>
              )}
            </span>
            <ChevronDown
              size={14}
              aria-hidden
              className={`shrink-0 opacity-60 ${expanded ? "rotate-180" : ""}`}
            />
          </button>

          {expanded && notification.body && (
            <div
              data-testid="chat-ask-body"
              className={`border-t px-3 py-2 leading-relaxed whitespace-pre-wrap break-words ${bodyText}`}
              style={edge}
            >
              {notification.body}
            </div>
          )}

          {state.phase === "sending" && (
            <div
              role="status"
              data-testid="chat-ask-sending"
              className="border-t px-3 py-2 text-[11px]"
              style={{ ...edge, color: "var(--ct-muted-fg)" }}
            >
              Sending <strong className="font-medium" style={{ color: "var(--ct-fg)" }}>{state.chosen}</strong>…
            </div>
          )}
          {state.error && (
            <div
              role="alert"
              className="border-t px-3 py-2 text-[11px]"
              style={{ ...edge, color: "var(--ct-red)" }}
            >
              {state.error}
            </div>
          )}
          {state.phase === "resolved" ? (
            <div
              data-testid="chat-ask-resolved"
              className="border-t px-3 py-2 text-[11px]"
              style={{ ...edge, color: "var(--ct-muted-fg)" }}
            >
              Resolved in tab.
            </div>
          ) : state.phase === "answered" ? (
            <div
              data-testid="chat-ask-answered"
              className="flex items-center gap-1.5 border-t px-3 py-2 text-[11px]"
              style={{ ...edge, color: "var(--ct-muted-fg)" }}
            >
              <Check
                size={13}
                strokeWidth={2.2}
                aria-hidden
                className="shrink-0"
                style={{ color: "var(--ct-green)" }}
              />
              <span>
                Answered{" "}
                <strong className="font-medium" style={{ color: "var(--ct-fg)" }}>
                  {state.chosen}
                </strong>
              </span>
            </div>
          ) : state.phase === "undeliverable" ? (
            <div
              data-testid="chat-ask-undeliverable"
              className="border-t px-3 py-2 text-[11px]"
              style={{ ...edge, color: "var(--ct-muted-fg)" }}
            >
              The tab that asked is gone — this can no longer be answered.
            </div>
          ) : (
            choices.length > 0 && (
              <div className="flex flex-wrap items-center gap-2 border-t px-3 py-2" style={edge}>
                {choices.map((choice, index) => (
                  <button
                    key={`${choice.label}-${index}`}
                    type="button"
                    data-testid="chat-ask-choice"
                    disabled={!isChoiceEnabled(state, choice.label, recorded)}
                    aria-pressed={state.chosen === choice.label}
                    aria-label={choiceAccessibleName(level, notification.title, choice.label)}
                    onClick={() => choose(choice.label)}
                    className="min-h-7 rounded-md border px-3 text-xs transition-colors hover:brightness-110"
                    style={
                      index === 0
                        ? {
                            borderColor: "var(--ct-accent)",
                            background: "color-mix(in srgb, var(--ct-surface) 80%, var(--ct-accent) 20%)",
                            color: "var(--ct-fg)",
                          }
                        : {
                            borderColor: "var(--ct-border)",
                            background: "transparent",
                            color: "var(--ct-muted-fg)",
                          }
                    }
                  >
                    {choice.label}
                  </button>
                ))}
              </div>
            )
          )}
        </div>
      </div>

      {recorded && askedFrom && slot?.delivered && (
        <SentMessageBubble
          inv={sentInvocation(askedFrom, recorded)}
          resolveTabName={resolveTabName}
          fontScale={fontScale}
        />
      )}
    </>
  );
}
