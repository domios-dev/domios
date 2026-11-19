import { useState, type MouseEvent } from "react";
import { BookOpen, ChevronRight } from "lucide-react";
import { useGuideNavigation } from "@codeterm/chat-ui/GuideDirective";
import { nativeBridge } from "../../lib/nativeBridge";
import { cleanMachineName } from "../../lib/machineIdentity";
import { useMeshMachineIdentity } from "../../lib/useMeshMachineIdentities";
import { Button } from "../shared/Button";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { MachineGlyph } from "../shared/MachineGlyph";
import { MachineJoinAction } from "./AddPeerPanel";
import { MachineSessionControls } from "./MachineSessionControls";
import type { CanvasAccount } from "./MeshCanvas";
import type { MeshOverviewState } from "./meshOverviewModel";
import type { CanvasTailscale } from "./TailscaleNodes";
import "./meshOverview.css";

type Overview = Exclude<MeshOverviewState, { kind: "inventory" }>;

function MachineNode({ name, status, tone, peerId, onSelect, selected }: {
  name: string;
  status: string;
  tone?: "online" | "muted" | "warning";
  peerId?: string | null;
  onSelect?: () => void;
  selected?: boolean;
}) {
  const identity = useMeshMachineIdentity(peerId, peerId !== null);
  const body = <>
    <span className="neighbor-me__ring mesh-overview__ring"><MachineGlyph {...identity} size="tile" /></span>
    <span className="neighbor-node__name">{name}</span>
    <span className="mesh-overview__status" data-tone={tone}>{status}</span>
  </>;
  return onSelect
    ? <button type="button" className="mesh-overview__node" aria-expanded={selected} onClick={onSelect}>{body}</button>
    : <div className="mesh-overview__node">{body}</div>;
}

export function MeshOverview({ state, account, name, tailscale }: {
  state: Overview;
  account: CanvasAccount;
  name: string;
  tailscale?: CanvasTailscale;
}) {
  const guide = useGuideNavigation();
  const [selected, setSelected] = useState<string | null>(null);
  const openGuide = (event?: MouseEvent<HTMLAnchorElement>) => {
    if (!guide || (event && (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey))) return;
    event?.preventDefault();
    guide.onOpen("mesh");
  };
  const localName = cleanMachineName(name) ?? "This machine";
  if (state.kind === "unreachable") {
    const { machine } = state;
    const remoteName = cleanMachineName(machine.name) ?? "Machine";
    const expanded = selected === machine.key;
    return <div className="mesh-overview__content">
      <div className="mesh-overview__nodes">
        <MachineNode name={localName} tone="online" status="online" />
        <span className="mesh-overview__edge" aria-hidden="true" />
        <MachineNode name={remoteName} tone="muted" status="not reachable" peerId={machine.pairInstanceId}
          selected={expanded} onSelect={() => setSelected(expanded ? null : machine.key)} />
      </div>
      {expanded && <div className="mesh-overview__callout ct-card ct-card--static">
        <InlineErrorNotice description={{ title: `${remoteName} isn't reachable`, cause: `It's signed in, but Tailscale is off or needs permission on ${remoteName}.`, raw: machine.detail, actions: [] }}>
          <Button size="sm" variant="primary" onClick={() => openGuide()}>Open Tailscale setup</Button>
        </InlineErrorNotice>
      </div>}
    </div>;
  }
  const signedOut = state.kind === "signed_out";
  const off = state.kind === "tailscale_off";
  return <div className="mesh-overview__content">
    <div className="mesh-overview__local">
      <MachineNode name={localName} tone={signedOut ? undefined : off ? "warning" : "online"}
        status={signedOut ? "This machine" : off ? "Tailscale is off" : "online · this machine"} />
      <div className="mesh-overview__identity"><MachineSessionControls name={localName} busy={false} identityPeer={null} rename={account.onRenameSelf ? { displayName: account.selfDisplayName, save: account.onRenameSelf } : undefined} /></div>
    </div>
    <div className="neighbor-me__signin mesh-overview__copy" role="status">
      <p className="neighbor-me__title">{signedOut ? "Sign in to connect your machines" : off ? "Turn on Tailscale to reach your machines" : "No other machines yet"}</p>
      <p className="neighbor-me__hint">{signedOut ? "Sign in with the same account on each machine. It joins here automatically." : off
        ? "This machine is signed in. Your other machines appear once Tailscale is on and allowed."
        : "Sign in on another machine with this account. It appears here on its own."}</p>
      {signedOut && <MachineJoinAction compact signingIn={account.signingIn} onSignIn={account.onSignIn} onCancel={account.cancelSignIn} />}
      {off && <Button variant="primary" size="sm" onClick={() => {
        if (tailscale?.state.kind === "not_installed") tailscale.onInstall();
        else void nativeBridge.openUrl("https://tailscale.com/docs/install", "tailscale");
      }}>Open Tailscale</Button>}
      <a href="https://tailscale.com/docs/install" className="mesh-overview__guide"
        onClick={openGuide} aria-label="Open the Mesh guide: Tailscale setup">
        <BookOpen size={13} aria-hidden="true" />
        {off ? "How Mesh uses Tailscale" : "Machine not showing? Check the Tailscale setup"}
        <ChevronRight size={13} aria-hidden="true" />
      </a>
    </div>
  </div>;
}
