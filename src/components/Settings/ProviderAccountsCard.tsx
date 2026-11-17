import { useEffect, useRef, useState } from "react";
import { Plus, Users } from "lucide-react";
import type { ProviderDescriptor } from "../../lib/types";
import type { ProviderAccountsAction, ProviderAccountsSnapshot } from "../../hooks/providerAccountsTypes";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { Loader } from "../shared/Loader";
import { RuntimeLoginActions } from "../Terminal/RuntimeLoginActions";
import { ProviderAccountRow } from "./ProviderAccountRow";
import { SettingsCard, Toggle } from "./shared";

interface ProviderAccountsCardProps {
  provider: ProviderDescriptor;
  snapshot: ProviderAccountsSnapshot | null;
  loading: boolean;
  error: string | null;
  onAction: (action: ProviderAccountsAction) => Promise<void>;
  onRefresh: () => void;
}

function useAccountCardControls(snapshot: ProviderAccountsSnapshot | null, onAction: ProviderAccountsCardProps["onAction"]) {
  const list = useRef<HTMLDivElement>(null);
  const add = useRef<HTMLButtonElement>(null);
  const active = useRef(false);
  const mounted = useRef(false);
  const [pendingAction, setPendingAction] = useState<ProviderAccountsAction | null>(null);
  const [announcement, setAnnouncement] = useState("");
  const feedback = useRef<{ loaded: boolean; id?: string }>({ loaded: false });
  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);
  useEffect(() => {
    if (!snapshot || pendingAction) return;
    const next = snapshot.feedback;
    if (!feedback.current.loaded) {
      feedback.current = { loaded: true, id: next?.id };
      return;
    }
    if (!next || feedback.current.id === next.id) return;
    feedback.current.id = next.id;
    if (next.kind === "removed") {
      setAnnouncement(next.identity ? `Removed ${next.identity}` : "Removed account");
      const nextAccount = snapshot.accounts[next.position];
      const nextRow = Array.from(list.current?.querySelectorAll<HTMLElement>("[data-account-id]") ?? [])
        .find(row => row.dataset.accountId === nextAccount?.id);
      if (nextRow) nextRow.focus();
      else add.current?.focus();
      return;
    }
    const account = snapshot.accounts.find(row => row.id === next.accountId);
    if (!account) return;
    const identity = account.identity ?? account.name ?? "Account";
    setAnnouncement(next.kind === "added" ? `Added ${identity}` : `${identity} moved to position ${next.position + 1}`);
    Array.from(list.current?.querySelectorAll<HTMLElement>("[data-account-id]") ?? [])
      .find(row => row.dataset.accountId === next.accountId)?.focus();
  }, [snapshot, pendingAction]);
  const act = async (action: ProviderAccountsAction): Promise<boolean> => {
    if (active.current) return false;
    active.current = true;
    setPendingAction(action);
    try { await onAction(action); return mounted.current; }
    catch { return false; }
    finally {
      active.current = false;
      if (mounted.current) setPendingAction(null);
    }
  };
  return { list, add, busy: pendingAction !== null, pendingAction, announcement, act };
}

export function ProviderAccountsCard(props: ProviderAccountsCardProps): React.JSX.Element {
  return <ProviderAccountsContent key={props.provider.id} {...props} />;
}

function ProviderAccountsContent({ provider, snapshot: supplied, loading, error, onAction, onRefresh }: ProviderAccountsCardProps) {
  const snapshot = supplied?.providerId === provider.id ? supplied : null;
  const controls = useAccountCardControls(snapshot, onAction);
  const adding = snapshot?.adding;
  const name = provider.display_name;
  const continuation = snapshot?.continuation;
  const toggleDescription = continuation === "unsupported"
    ? `${name} can't move a running conversation to another account. New tabs still start on the next account with room.`
    : continuation === "unverified"
      ? `Moving a running ${name} conversation to another account isn't confirmed yet. New tabs still start on the next account with room.`
      : "When an account reaches its limit, its tabs continue on the next one.";

  return <SettingsCard icon={<Users size={16} />} title="Accounts"
    description="Tabs start on the first account and move down the list when one reaches its limit.">
    <div role="status" aria-live="polite" aria-atomic="true" className="sr-only">{controls.announcement}</div>
    {error && <InlineErrorNotice error={error} operation="updating accounts" actions={[{ label: "Try again", onClick: onRefresh, disabled: controls.busy }]} />}
    {!snapshot && loading && <Loader label="Loading accounts" />}
    {!snapshot && !loading && !error && <button type="button" className="ct-bar-pill min-h-11 self-start px-3" onClick={onRefresh}>Refresh accounts</button>}
    {snapshot && <>
      <div ref={controls.list} role="list" aria-label={`${name} accounts`} aria-busy={controls.busy} className="ct-rows !p-0">
        {snapshot.accounts.map((account, position) => <ProviderAccountRow key={account.id} account={account} providerId={provider.id} providerName={name}
          iconSvg={provider.icon_svg} position={position} count={snapshot.accounts.length} busy={controls.busy} continuation={snapshot.continuation}
          retryingRemoval={controls.pendingAction?.action === "remove" && controls.pendingAction.accountId === account.id && account.status === "removing"}
          onAction={controls.act} onRefresh={onRefresh} />)}
        <div role="listitem">
          <button ref={controls.add} type="button" disabled={controls.busy || Boolean(adding)} onClick={() => { void controls.act({ action: "add" }); }}
            className="ct-row flex min-h-11 w-full items-center gap-2 px-2 text-left text-[13px] text-[var(--ct-muted-fg)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-border-focus)] disabled:opacity-50">
            <Plus size={14} aria-hidden="true" />Add account
          </button>
        </div>
      </div>
      {adding?.status === "signingIn" && <div>
        <RuntimeLoginActions providerId={provider.id} tabId={adding.context.tabId} state={adding.context.state} iconSvg={provider.icon_svg}
          variant="inline" title={`Add a ${name} account`} hint={adding.context.state.notice ?? (adding.context.state.login?.status === "pending" ? "Finish signing in to add this account." : "Choose how to sign in to the new account.")} onCheck={onRefresh} />
        {adding.context.state.login?.status !== "pending" && <button type="button" className="ct-bar-pill min-h-11 px-3 text-xs text-[var(--ct-muted-fg)]"
          disabled={controls.busy} onClick={() => { void controls.act({ action: "cancelAdd" }); }}>Cancel</button>}
      </div>}
      {adding?.status === "duplicate" && <div role="status" aria-live="polite" className="text-xs text-[var(--ct-muted-fg)]">
        <p>This is {adding.identity ?? "an account"}, which is already in the list.</p>
        <button type="button" className="ct-bar-pill min-h-11 px-3" disabled={controls.busy} onClick={() => { void controls.act({ action: "retryAdd" }); }}>Use a different account</button>
      </div>}
      {adding?.status === "failed" && <InlineErrorNotice description={{ title: adding.failureKind === "sharedFileLinkUnsupported" ? "Shared file linking is unsupported on this machine." : "Couldn't add the account.", cause: "", actions: [], raw: adding.details ?? adding.error ?? "" }}
        actions={[{ label: "Try again", onClick: () => { void controls.act({ action: "retryAdd" }); }, disabled: controls.busy }]} />}
      {snapshot.accounts.length > 1 && <Toggle label="Switch accounts automatically" checked={continuation === "proven" && snapshot.automaticSwitch}
        disabled={controls.busy || continuation !== "proven"} description={toggleDescription}
        onChange={enabled => { void controls.act({ action: "automaticSwitch", enabled }); }} />}
      {snapshot.workspaceOverrides.map(override => {
        const account = snapshot.accounts.find(row => row.id === override.accountId);
        if (!account) return null;
        return <div key={override.workspaceId} className="flex min-w-0 items-center justify-between gap-2 text-xs text-[var(--ct-muted-fg)]">
          <span className="min-w-0 break-words">{override.workspaceName} workspace starts on {account.identity ?? account.name ?? "the usual sign-in"}</span>
          <button type="button" className="ct-bar-pill min-h-11 shrink-0 px-3" aria-label={`Reset account for ${override.workspaceName}`} disabled={controls.busy}
            onClick={() => { void controls.act({ action: "resetWorkspace", workspaceId: override.workspaceId }); }}>Reset</button>
        </div>;
      })}
    </>}
  </SettingsCard>;
}
