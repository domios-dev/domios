import { useRef, useState } from "react";
import { UNKNOWN_MACHINE } from "../../lib/machineIdentity";
import type { DuplicateMachineName } from "../../lib/registryMachineNames";
import { useMeshMachineIdentity } from "../../lib/useMeshMachineIdentities";
import { Button } from "../shared/Button";
import type { CanvasAccount } from "./MeshCanvas";
import { MachineIdentityPopover } from "./MachineIdentityEditor";

function DuplicateNameNotice({ group, account }: { group: DuplicateMachineName; account: CanvasAccount }) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLButtonElement>(null);
  const own = account.selfInstallationId && group.installations.includes(account.selfInstallationId) && account.onRenameSelf;
  const remote = [...account.canvas.peerFooters.values()].map(({ row }) => row)
    .find(row => group.installations.includes(row.key) && row.online && row.target && row.pairInstanceId);
  const peerId = own ? undefined : remote?.pairInstanceId ?? undefined;
  const identity = useMeshMachineIdentity(peerId);
  const renamePeer = account.actions.onRename;
  const save = own || (remote && renamePeer ? (name: string) => renamePeer(remote, name) : undefined);
  const display = own ? account.selfName : remote?.name;
  return <div className="ct-card ct-card--static flex items-center gap-2 px-3 py-1">
    <p role="status" className="m-0">{group.installations.length === 2 ? "Two" : group.installations.length} machines are named {group.name}</p>
    <Button ref={anchor} size="compact" disabled={!save} title={save ? undefined : "Open an online machine to rename it"}
      onClick={() => setOpen(true)}>Rename</Button>
    {open && save && <MachineIdentityPopover name={display ?? group.name} initial={identity ?? UNKNOWN_MACHINE} peerId={peerId}
      anchor={anchor} focusName rename={{ displayName: own ? account.selfDisplayName : remote?.displayName, save }}
      onClose={() => { setOpen(false); anchor.current?.focus({ preventScroll: true }); }} />}
  </div>;
}

export function DuplicateMachineNotices({ account }: { account?: CanvasAccount }) {
  if (!account?.duplicateNames?.length) return null;
  return <div data-peer-card className="absolute bottom-12 left-3 z-10 grid max-w-[min(360px,calc(100%-24px))] gap-1 text-xs">
    {account.duplicateNames.map(group => <DuplicateNameNotice key={group.name} group={group} account={account} />)}
  </div>;
}
