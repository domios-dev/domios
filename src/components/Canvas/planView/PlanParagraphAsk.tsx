import {
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import { ExternalLink, MessageCircle, X } from "lucide-react";
import type { Tab } from "../../../lib/types";
import type { Plan, Section } from "../types";
import { useReducedMotion } from "../missionControl/useReducedMotion";
import { sendTrackedChatMessage } from "../../Terminal/chatSendStatus";
import { PlanAskExchange } from "./PlanAskExchange";
import {
  PLAN_ASK_BLOCK_SELECTOR,
  PLAN_ASK_BODY_SELECTOR,
  paragraphTarget,
  selectionTarget,
  type PlanAskContext,
  type PlanAskSource,
  type PlanAskTarget,
  type PlanDocumentKind,
} from "./planAskMapping";
import {
  composePlanAskMessage,
  createPlanAskId,
  resolvePlanAskOwner,
  type PlanAskOwner,
  type PlanAskReadyOwner,
} from "./planAskModel";

const ASK_BLOCK_ATTR = "data-plan-ask-block";

type Motion = "glide" | "snap";

interface AskAnchor {
  target: PlanAskTarget;
  top: number;
  key?: number;
}

export interface PlanParagraphAskProps {
  kind: PlanDocumentKind;
  plan: Plan;
  sections: Section[];
  tabs?: Record<string, Tab>;
  onOpenTab?: (tabId: string) => void;
  children: ReactNode;
}

function indexSections(sections: Section[], out = new Map<string, PlanAskSource>()): Map<string, PlanAskSource> {
  for (const section of sections) {
    out.set(section.id, { id: section.id, title: section.title, body: section.body ?? "" });
    indexSections(section.subsections ?? [], out);
  }
  return out;
}

function isEnter(event: ReactKeyboardEvent): boolean {
  return event.code === "Enter" || event.code === "NumpadEnter";
}

function askLabel(target: PlanAskTarget): string {
  return target.mode === "selection" ? "Ask about selected text" : "Ask about this paragraph";
}

function focusBlock(block: HTMLElement | null) {
  if (!block?.isConnected) return;
  if (!block.hasAttribute("tabindex")) block.tabIndex = -1;
  block.focus({ preventScroll: true });
}

export function PlanParagraphAsk({ kind, plan, sections, tabs, onOpenTab, children }: PlanParagraphAskProps) {
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const restoreFocusRef = useRef<HTMLElement | null>(null);
  const openSerialRef = useRef(0);
  const motion: Motion = useReducedMotion() ? "snap" : "glide";
  const [hover, setHover] = useState<AskAnchor | null>(null);
  const [selection, setSelection] = useState<AskAnchor | null>(null);
  const [open, setOpen] = useState<AskAnchor | null>(null);
  const active = open ?? selection ?? hover;
  const owner = useMemo(() => resolvePlanAskOwner(plan, tabs), [plan, tabs]);
  const sectionIndex = useMemo(() => indexSections(sections), [sections]);
  const context = useMemo<PlanAskContext>(
    () => ({
      planId: plan.id,
      document: kind,
      specId: kind === "spec" ? plan.primary_spec_id ?? null : null,
      section: (id) => sectionIndex.get(id) ?? null,
    }),
    [plan.id, plan.primary_spec_id, kind, sectionIndex],
  );

  const topOf = useCallback((rect: DOMRect | null) => {
    const root = rootRef.current;
    if (!root || !rect) return 0;
    return Math.max(0, rect.top - root.getBoundingClientRect().top);
  }, []);

  useLayoutEffect(() => {
    const root = rootRef.current;
    if (!root) return;
    for (const body of Array.from(root.querySelectorAll(PLAN_ASK_BODY_SELECTOR))) {
      for (const block of Array.from(body.querySelectorAll<HTMLElement>(PLAN_ASK_BLOCK_SELECTOR))) {
        if (block.hasAttribute(ASK_BLOCK_ATTR) || !block.hasAttribute("data-src-start")) continue;
        block.setAttribute(ASK_BLOCK_ATTR, "");
        block.setAttribute("aria-keyshortcuts", "Enter");
        block.tabIndex = 0;
      }
    }
  });

  const blockAnchor = useCallback(
    (element: EventTarget | null): AskAnchor | null => {
      if (!(element instanceof Element)) return null;
      const block = element.closest<HTMLElement>(PLAN_ASK_BLOCK_SELECTOR);
      if (!block || !block.closest(PLAN_ASK_BODY_SELECTOR) || !rootRef.current?.contains(block)) return null;
      return { target: paragraphTarget(block, context), top: topOf(block.getBoundingClientRect()) };
    },
    [context, topOf],
  );

  useEffect(() => {
    if (open) return;
    const onSelectionChange = () => {
      const root = rootRef.current;
      const current = document.getSelection?.();
      if (!root || !current || current.rangeCount === 0 || current.isCollapsed) {
        setSelection(null);
        return;
      }
      const range = current.getRangeAt(0);
      const target = selectionTarget(range, root, context);
      const rect = typeof range.getBoundingClientRect === "function" ? range.getBoundingClientRect() : null;
      setSelection(target ? { target, top: topOf(rect) } : null);
    };
    document.addEventListener("selectionchange", onSelectionChange);
    return () => document.removeEventListener("selectionchange", onSelectionChange);
  }, [open, context, topOf]);

  useEffect(() => {
    if (open) return;
    const block = restoreFocusRef.current;
    restoreFocusRef.current = null;
    focusBlock(block);
  }, [open]);

  const openAsk = useCallback((anchor: AskAnchor) => {
    openSerialRef.current += 1;
    setOpen({ ...anchor, key: openSerialRef.current });
  }, []);

  const closeAsk = useCallback(() => {
    restoreFocusRef.current = open?.target.block ?? null;
    setSelection(null);
    setOpen(null);
  }, [open]);

  const showBlock = (element: EventTarget | null) => {
    if (open) return;
    const next = blockAnchor(element);
    if (next) setHover(next);
  };

  const onKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (open || !(event.target instanceof HTMLElement) || !event.target.hasAttribute(ASK_BLOCK_ATTR)) return;
    if (isEnter(event)) {
      const next = selection ?? blockAnchor(event.target);
      if (!next) return;
      event.preventDefault();
      openAsk(next);
    } else if (event.code === "Tab" && !event.shiftKey && triggerRef.current) {
      event.preventDefault();
      triggerRef.current.focus();
    }
  };

  const onTriggerKeyDown = (event: ReactKeyboardEvent<HTMLButtonElement>) => {
    const block = active?.target.block ?? null;
    if (!block || open) return;
    if (event.code === "Escape" || (event.code === "Tab" && event.shiftKey)) {
      event.preventDefault();
      focusBlock(block);
      return;
    }
    if (event.code !== "Tab") return;
    const blocks = Array.from(rootRef.current?.querySelectorAll<HTMLElement>(`[${ASK_BLOCK_ATTR}]`) ?? []);
    const next = blocks[blocks.indexOf(block) + 1];
    if (!next) return;
    event.preventDefault();
    next.focus();
  };

  return (
    <div
      ref={rootRef}
      data-testid="plan-ask-layer"
      className="plan-document-sections plan-ask-layer"
      onMouseOver={(event) => showBlock(event.target)}
      onFocus={(event) => showBlock(event.target)}
      onMouseLeave={() => {
        if (!open && !rootRef.current?.contains(document.activeElement)) setHover(null);
      }}
      onKeyDown={onKeyDown}
    >
      {children}
      {active && (
        <button
          ref={triggerRef}
          type="button"
          data-testid="plan-ask-trigger"
          data-ask-mode={active.target.mode}
          data-motion={motion}
          className="plan-ask-trigger"
          style={{ transform: `translateY(${active.top}px)` }}
          aria-label={askLabel(active.target)}
          title={askLabel(active.target)}
          aria-haspopup="dialog"
          aria-expanded={open !== null}
          onMouseDown={(event) => event.preventDefault()}
          onClick={() => (open ? closeAsk() : openAsk(active))}
          onKeyDown={onTriggerKeyDown}
        >
          <MessageCircle size={14} strokeWidth={2} aria-hidden />
        </button>
      )}
      {open && (
        <PlanAskPopover
          key={open.key}
          anchor={open}
          owner={owner}
          motion={motion}
          onClose={closeAsk}
          onOpenTab={onOpenTab}
        />
      )}
    </div>
  );
}

interface PlanAskPopoverProps {
  anchor: AskAnchor;
  owner: PlanAskOwner;
  motion: Motion;
  onClose: () => void;
  onOpenTab?: (tabId: string) => void;
}

function PlanAskPopover({ anchor, owner, motion, onClose, onOpenTab }: PlanAskPopoverProps) {
  const inputId = useId();
  const dialogRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const mountedRef = useRef(true);
  const sendingRef = useRef(false);
  const [question, setQuestion] = useState("");
  const [phase, setPhase] = useState<"compose" | "sending" | "error">("compose");
  const [sent, setSent] = useState<{ askId: string; question: string; owner: PlanAskReadyOwner } | null>(null);
  const { target } = anchor;
  const label = askLabel(target);
  const ready = owner.status === "ready" ? owner : null;
  const reference = target.status === "mapped" ? target.reference : null;
  const quote = reference ? reference.quote : target.status === "unmapped" ? target.quote : "";
  const exchangeOwner = sent && ready && ready.tabId === sent.owner.tabId ? ready : sent?.owner ?? null;

  useEffect(() => {
    mountedRef.current = true;
    (inputRef.current ?? dialogRef.current)?.focus();
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const send = async () => {
    const text = question.trim();
    if (!ready || !reference || !text || sent || sendingRef.current) return;
    sendingRef.current = true;
    const askId = createPlanAskId();
    setPhase("sending");
    try {
      await sendTrackedChatMessage(ready.tabId, askId, composePlanAskMessage(reference, text, askId));
      if (mountedRef.current) setSent({ askId, question: text, owner: ready });
    } catch {
      if (mountedRef.current) setPhase("error");
    } finally {
      sendingRef.current = false;
    }
  };

  return (
    <div
      ref={dialogRef}
      role="dialog"
      aria-label={label}
      tabIndex={-1}
      data-testid="plan-ask-popover"
      data-ask-mode={target.mode}
      data-motion={motion}
      className="plan-ask-popover"
      style={{ top: anchor.top }}
      onKeyDown={(event) => {
        if (event.code !== "Escape") return;
        event.preventDefault();
        event.stopPropagation();
        onClose();
      }}
    >
      <header className="plan-ask-header">
        <span className="plan-ask-title">Ask {owner.status === "missing" ? "Orchestrator" : owner.name}</span>
        {ready && onOpenTab && (
          <button
            type="button"
            data-testid="plan-ask-expand"
            className="plan-ask-icon-button"
            aria-label={`Open ${ready.name} chat`}
            title="Open chat"
            onClick={() => onOpenTab(ready.tabId)}
          >
            <ExternalLink size={13} aria-hidden />
          </button>
        )}
        <button
          type="button"
          data-testid="plan-ask-close"
          className="plan-ask-icon-button"
          aria-label="Close"
          title="Close"
          onClick={onClose}
        >
          <X size={13} aria-hidden />
        </button>
      </header>
      <blockquote data-testid="plan-ask-quote" className="plan-ask-quote">{quote}</blockquote>
      {owner.status === "missing" && (
        <p role="status" data-testid="plan-ask-owner-missing" className="plan-ask-note">
          No Orchestrator owns this plan. Nothing was sent.
        </p>
      )}
      {owner.status === "offline" && (
        <p role="status" data-testid="plan-ask-owner-missing" data-owner-tab-id={owner.tabId} className="plan-ask-note">
          {owner.name} isn't running. Nothing was sent.
        </p>
      )}
      {!reference && (
        <p role="status" data-testid="plan-ask-unmapped" className="plan-ask-note">
          This text can't be traced to its source. Select text inside one section.
        </p>
      )}
      {ready?.busy && !sent && (
        <p role="status" data-testid="plan-ask-busy" className="plan-ask-note">
          {ready.name} is busy. The reply appears here.
        </p>
      )}
      {ready && reference && !sent && (
        <div className="plan-ask-compose">
          <label htmlFor={inputId} className="plan-ask-label">{label}</label>
          <textarea
            id={inputId}
            ref={inputRef}
            data-testid="plan-ask-input"
            className="plan-ask-input"
            rows={2}
            value={question}
            disabled={phase === "sending"}
            onChange={(event) => setQuestion(event.target.value)}
            onKeyDown={(event) => {
              if (!isEnter(event) || !(event.metaKey || event.ctrlKey)) return;
              event.preventDefault();
              void send();
            }}
          />
          {phase === "error" && (
            <p role="alert" data-testid="plan-ask-error" className="plan-ask-note">
              Couldn't reach {ready.name}.
            </p>
          )}
          <div className="plan-ask-actions">
            <button type="button" data-testid="plan-ask-cancel" className="plan-ask-button" onClick={onClose}>
              Cancel
            </button>
            <button
              type="button"
              data-testid="plan-ask-send"
              className="plan-ask-button plan-ask-button-primary"
              disabled={!question.trim() || phase === "sending"}
              onClick={() => void send()}
            >
              {phase === "sending" ? "Sending…" : "Ask"}
            </button>
          </div>
        </div>
      )}
      {sent && exchangeOwner && (
        <>
          <p data-testid="plan-ask-question" className="plan-ask-question">{sent.question}</p>
          <PlanAskExchange owner={exchangeOwner} busy={ready?.tabId === exchangeOwner.tabId && ready.busy} askId={sent.askId} />
        </>
      )}
    </div>
  );
}
