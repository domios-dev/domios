import { useState, type ComponentProps } from "react";
import { createPortal } from "react-dom";
import { WebAccessPanel } from "./WebAccessPanel";

type Props = Omit<ComponentProps<typeof WebAccessPanel>, "embedded" | "onClose" | "onExpand">;

export function WebNodeCard(props: Props) {
  const [expanded, setExpanded] = useState(false);
  return <>
    <WebAccessPanel {...props} embedded onClose={() => {}} onExpand={() => setExpanded(true)} />
    {expanded && createPortal(<WebAccessPanel {...props} qrSize={280} onClose={() => setExpanded(false)} />, document.body)}
  </>;
}
