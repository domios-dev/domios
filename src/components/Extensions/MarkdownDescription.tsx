/**
 * Safe markdown renderer for untrusted plugin README/description. No `rehype-raw`
 * (embedded HTML stays inert), dangerous link protocols stripped, and remote images
 * aren't fetched (a README could beacon arbitrary hosts) — rendered as a caption instead.
 */
import Markdown from "react-markdown";
import { MarkdownLink } from "@codeterm/chat-ui/MarkdownLink";
import remarkGfm from "remark-gfm";
import type { CSSProperties } from "react";

const baseText: CSSProperties = { fontSize: 13, lineHeight: 1.6, color: "var(--ct-fg)" };

export function MarkdownDescription({ markdown }: { markdown: string }) {
  return (
    <div style={{ ...baseText, wordBreak: "break-word" }} data-testid="extensions-markdown-description">
      <Markdown
        remarkPlugins={[remarkGfm]}
        // No rehypePlugins → no raw HTML. Keep it that way.
        components={{
          h1: ({ children }) => <h2 style={{ fontSize: 17, fontWeight: 640, margin: "14px 0 8px" }}>{children}</h2>,
          h2: ({ children }) => <h3 style={{ fontSize: 15, fontWeight: 620, margin: "14px 0 6px" }}>{children}</h3>,
          h3: ({ children }) => <h4 style={{ fontSize: 13.5, fontWeight: 600, margin: "12px 0 6px" }}>{children}</h4>,
          p: ({ children }) => <p style={{ margin: "0 0 10px" }}>{children}</p>,
          a: ({ href, children }) => (
            <MarkdownLink href={href} style={{ color: "var(--ct-accent)" }}>
              {children}
            </MarkdownLink>
          ),
          ul: ({ children }) => <ul style={{ margin: "0 0 10px", paddingLeft: 20 }}>{children}</ul>,
          ol: ({ children }) => <ol style={{ margin: "0 0 10px", paddingLeft: 20 }}>{children}</ol>,
          li: ({ children }) => <li style={{ margin: "2px 0" }}>{children}</li>,
          // GFM table overrides — without them tables fall back to unstyled browser defaults.
          table: ({ children }) => (
            <div style={{ overflowX: "auto", margin: "0 0 12px" }}>
              {/* minWidth over width:100% — a narrow drawer otherwise crushes columns to one letter instead of scrolling. */}
              <table style={{ borderCollapse: "collapse", fontSize: 12.5, minWidth: "max-content" }}>{children}</table>
            </div>
          ),
          thead: ({ children }) => (
            <thead style={{ background: "color-mix(in srgb, var(--ct-fg) 4%, var(--ct-bg))" }}>{children}</thead>
          ),
          th: ({ children }) => (
            <th
              style={{
                border: "1px solid var(--ct-border-subtle)",
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
                border: "1px solid var(--ct-border-subtle)",
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
                fontFamily: "var(--ct-font-mono)",
                fontSize: 12,
                background: "color-mix(in srgb, var(--ct-fg) 5%, var(--ct-bg))",
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
                background: "color-mix(in srgb, var(--ct-fg) 4%, var(--ct-bg))",
                border: "1px solid var(--ct-border-subtle)",
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
                borderLeft: "3px solid var(--ct-border)",
                margin: "0 0 10px",
                padding: "2px 0 2px 12px",
                color: "var(--ct-muted-fg)",
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
