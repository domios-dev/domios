import { useEffect, useRef, useState, type HTMLAttributes, type ReactNode } from "react";
import "./teamTemplates.css";

/**
 * A footer pinned to the session dialog's bottom edge. Its fade shows only while content scrolls beneath it, so the
 * last line above an unscrolled footer keeps its full colour. The sentinel after it marks the end of the content.
 */
export function PinnedFooter({ className = "", sentinelClassName = "-mt-px", children, ...rest }: HTMLAttributes<HTMLDivElement> & {
  children: ReactNode;
  /** Cancels the sentinel's own height plus any flex gap before it, so the footer still reaches the dialog edge. */
  sentinelClassName?: string;
}) {
  const sentinel = useRef<HTMLSpanElement>(null);
  const [covering, setCovering] = useState(false);
  useEffect(() => {
    const node = sentinel.current;
    if (!node || typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver(([entry]) => setCovering(!entry.isIntersecting));
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  return <>
    <div {...rest} className={`template-pinned-footer sticky z-[1] ${className}`} data-covering={covering || undefined}>
      {covering && <span aria-hidden="true" className="pointer-events-none absolute inset-x-0 -top-6 h-6 bg-gradient-to-t from-[var(--ct-surface)] to-transparent" />}
      {children}
    </div>
    <span ref={sentinel} aria-hidden="true" className={`block h-px ${sentinelClassName}`} />
  </>;
}
