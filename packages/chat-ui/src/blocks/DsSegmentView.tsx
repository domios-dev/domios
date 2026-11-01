import React from "react";
import { localArtifactFromBlock, type DsSegment } from "@codeterm/chat-core";
import { HtmlPreviewBlock } from "../HtmlPreviewBlock";
import { LocalArtifactBlock } from "../LocalArtifactBlock";
import { RichMarkdown } from "../MarkdownBody";
import { CodeBlock, LONG_CODE_PREVIEW_LINES, MERMAID_BLOCK_MAX_HEIGHT } from "../CodeBlock";
import { renderDsCard } from "./registry";

export const DsSegmentView = React.memo(function DsSegmentView({ segment, fontScale }: { segment: DsSegment; fontScale?: number }) {
  if (segment.type === "block") {
    const artifact = localArtifactFromBlock(segment.block);
    if (segment.block.kind === "html") return <HtmlPreviewBlock text={segment.block.html} />;
    if (artifact) return <LocalArtifactBlock directive={artifact} renderMarkdown={(text) => <RichMarkdown text={text} fontScale={fontScale} localArtifacts={false} interactivePreviews={false} />} />;
    return <>{renderDsCard(segment.block)}</>;
  }
  if (segment.type === "diagram") return <CodeBlock text={segment.text} language="mermaid" maxHeight={MERMAID_BLOCK_MAX_HEIGHT} collapsible collapseAfterLines={LONG_CODE_PREVIEW_LINES} />;
  if (segment.type === "inert") return <pre className="my-2 whitespace-pre-wrap break-words text-xs"><code>{segment.text}</code></pre>;
  if (segment.type === "pending" || segment.type === "malformed") return <div role="status" data-ds-state={segment.type} className="my-2 text-xs text-[var(--ct-muted-fg)]">{segment.type === "pending" ? "Incomplete block" : "Block unavailable"}</div>;
  return null;
});
