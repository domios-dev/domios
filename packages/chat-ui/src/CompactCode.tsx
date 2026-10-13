import React, { useEffect, useRef, useState } from "react";
import { Check, Copy } from "lucide-react";
import { copyCodeText } from "./copyCodeText";

export function CompactCode({ text, block, children }: { text: string; block: boolean; children: React.ReactNode }) {
  const [touched, setTouched] = useState(false);
  const [copied, setCopied] = useState(false);
  const feedback = useRef<HTMLSpanElement>(null);
  useEffect(() => {
    if (!copied || !feedback.current?.animate) return;
    const animation = feedback.current.animate([{ opacity: 1 }, { opacity: 1, offset: 0.9 }, { opacity: 0 }], { duration: 1400 });
    animation.onfinish = () => setCopied(false);
    return () => animation.cancel();
  }, [copied]);
  return (
    <span className={`group/code relative ${block ? "block" : "inline"}`} onPointerDown={(event) => {
      if (event.pointerType === "touch") setTouched(true);
    }} onBlur={(event) => {
      if (!event.currentTarget.contains(event.relatedTarget)) setTouched(false);
    }}>
      {children}
      <button
        type="button"
        aria-label="Copy code"
        title={copied ? "Copied" : "Copy code"}
        className={`absolute right-0 top-0 z-10 inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-xs opacity-0 group-hover/code:opacity-100 group-focus-within/code:opacity-100 ${touched ? "opacity-100" : ""}`}
        style={{ color: "var(--ct-muted-fg)", background: "var(--ct-bg)", userSelect: "none" }}
        onClick={async (event) => {
          event.stopPropagation();
          await copyCodeText(text);
          setCopied(true);
        }}
      >
        {copied ? <Check size={12} /> : <Copy size={12} />}
        {copied && <span ref={feedback} role="status">Copied</span>}
      </button>
    </span>
  );
}
