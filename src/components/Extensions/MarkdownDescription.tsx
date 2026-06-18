/**
 * Safe markdown renderer for plugin README / description content.
 *
 * Security model: plugin markdown is untrusted. We render with react-markdown
 * WITHOUT `rehype-raw`, so raw embedded HTML in the markdown is treated as plain
 * text and never executed (no `dangerouslySetInnerHTML` path). react-markdown's
 * default URL transform also strips dangerous link protocols (`javascript:` etc).
 *
 * Images: remote images are intentionally NOT loaded. A plugin README could
 * otherwise beacon arbitrary hosts (tracking pixels) or pull unvetted content.
 * There is no backend-served plugin-asset URL for README assets yet, so we render
 * a small inline caption for any image instead of fetching it. When an asset
 * proxy lands, swap the `img` override to resolve local `./` paths through it.
 */
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { CSSProperties } from "react";

const baseText: CSSProperties = { fontSize: 13, lineHeight: 1.6, color: "var(--ct-fg)" };

export function MarkdownDescription({ markdown }: { markdown: string }) {
  return (
    <div style={{ ...baseText, wordBreak: "break-word" }}>
      <Markdown
        remarkPlugins={[remarkGfm]}
        // No rehypePlugins → no raw HTML. Keep it that way.
        components={{
          h1: ({ children }) => <h2 style={{ fontSize: 17, fontWeight: 640, margin: "14px 0 8px" }}>{children}</h2>,
          h2: ({ children }) => <h3 style={{ fontSize: 15, fontWeight: 620, margin: "14px 0 6px" }}>{children}</h3>,
          h3: ({ children }) => <h4 style={{ fontSize: 13.5, fontWeight: 600, margin: "12px 0 6px" }}>{children}</h4>,
          p: ({ children }) => <p style={{ margin: "0 0 10px" }}>{children}</p>,
          a: ({ href, children }) => (
            <a href={href} target="_blank" rel="noreferrer noopener" style={{ color: "var(--ct-accent, #818cf8)" }}>
              {children}
            </a>
          ),
          ul: ({ children }) => <ul style={{ margin: "0 0 10px", paddingLeft: 20 }}>{children}</ul>,
          ol: ({ children }) => <ol style={{ margin: "0 0 10px", paddingLeft: 20 }}>{children}</ol>,
          li: ({ children }) => <li style={{ margin: "2px 0" }}>{children}</li>,
          // GFM tables: react-markdown parses them (remark-gfm), but without these
          // overrides they fall back to unstyled browser defaults (cramped, no
          // borders). Mirror the chat's MarkdownBody table treatment.
          table: ({ children }) => (
            <div style={{ overflowX: "auto", margin: "0 0 12px" }}>
              <table style={{ borderCollapse: "collapse", fontSize: 12.5, width: "100%" }}>{children}</table>
            </div>
          ),
          thead: ({ children }) => (
            <thead style={{ background: "var(--ct-bg-elev, rgba(255,255,255,0.06))" }}>{children}</thead>
          ),
          th: ({ children }) => (
            <th
              style={{
                border: "1px solid var(--ct-border-subtle, rgba(255,255,255,0.12))",
                padding: "5px 9px",
                textAlign: "left",
                fontWeight: 600,
              }}
            >
              {children}
            </th>
          ),
          td: ({ children }) => (
            <td
              style={{
                border: "1px solid var(--ct-border-subtle, rgba(255,255,255,0.08))",
                padding: "5px 9px",
                verticalAlign: "top",
              }}
            >
              {children}
            </td>
          ),
          code: ({ children }) => (
            <code
              style={{
                fontFamily: '"JetBrains Mono", ui-monospace, monospace',
                fontSize: 12,
                background: "var(--ct-bg-elev, rgba(255,255,255,0.06))",
                borderRadius: 4,
                padding: "1px 5px",
              }}
            >
              {children}
            </code>
          ),
          pre: ({ children }) => (
            <pre
              style={{
                background: "var(--ct-bg-elev, rgba(255,255,255,0.05))",
                border: "1px solid var(--ct-border-subtle, rgba(255,255,255,0.08))",
                borderRadius: 8,
                padding: "10px 12px",
                overflowX: "auto",
                fontSize: 12,
                margin: "0 0 12px",
              }}
            >
              {children}
            </pre>
          ),
          blockquote: ({ children }) => (
            <blockquote
              style={{
                borderLeft: "3px solid var(--ct-border-default, rgba(255,255,255,0.15))",
                margin: "0 0 10px",
                padding: "2px 0 2px 12px",
                color: "var(--ct-muted)",
              }}
            >
              {children}
            </blockquote>
          ),
          // Images are not fetched — see file header. Render the alt text as a caption.
          img: ({ alt }) => (
            <span style={{ fontSize: 11.5, color: "var(--ct-muted)", fontStyle: "italic" }}>
              🖼 {alt || "image"} (not shown)
            </span>
          ),
        }}
      >
        {markdown}
      </Markdown>
    </div>
  );
}
