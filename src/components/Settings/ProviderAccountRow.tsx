import { useEffect, useRef, useState } from "react";
import { DeferredLoader } from "@codeterm/chat-ui/DeferredLoader";
import type { ProviderAccountRow as Account, ProviderAccountsAction, ProviderAccountsSnapshot } from "../../hooks/providerAccountsTypes";
import { BusyState } from "../shared/BusyState";
import { DropdownMenu, type DropdownMenuEntry } from "../shared/DropdownMenu";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { KebabGlyph, RowMenuButton } from "../shared/RowMenuButton";
import { RuntimeLoginActions } from "../Terminal/RuntimeLoginActions";

const quiet = "ct-bar-pill min-h-11 px-3 text-xs text-[var(--ct-muted-fg)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-border-focus)] disabled:opacity-50";

function accountStatus(account: Account, retryingRemoval: boolean): string {
  if (account.status === "checking") return "Checking sign-in";
  if (account.status === "probeFailed") return "Couldn't check sign-in";
  if (account.status === "unknown") return "";
  if (account.status === "exhausted") {
    const reset = account.resetsAtMs == null ? "" : `, resets ${new Date(account.resetsAtMs).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`;
    return `Limit reached${reset}`;
  }
  if (account.status === "signedOut") return "Signed out";
  if (account.status === "signingIn") return "Waiting for sign-in";
  if (account.status === "removing") {
    if (account.removalFailureReason != null && !retryingRemoval) return "couldn't finish removing";
    const remaining = account.tabsToFinish ?? account.tabsInUse;
    return retryingRemoval || remaining === 0 ? "Removing" : `Removing after ${remaining} tabs finish their turn`;
  }
  if (account.tabsInUse > 0) return `In use by ${account.tabsInUse} ${account.tabsInUse === 1 ? "tab" : "tabs"}`;
  return account.plan ?? (account.id === "default" ? "Your usual sign-in" : "");
}

function useRowEditor(account: Account, busy: boolean, removalBlocked: boolean) {
  const [mode, setMode] = useState<"rename" | "remove" | null>(null);
  const [draft, setDraft] = useState("");
  const menu = useRef<HTMLButtonElement>(null);
  const editor = useRef<HTMLInputElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const previous = useRef(mode);
  const noticeDismissal = useRef<string | null>(null);
  useEffect(() => {
    if (mode === "remove") cancel.current?.focus();
    if (mode === "rename") { editor.current?.focus(); editor.current?.select(); }
    if (mode === null && previous.current !== null) menu.current?.focus();
    previous.current = mode;
  }, [mode]);
  useEffect(() => {
    if ((removalBlocked && mode === "remove") || (account.status === "removing" && mode !== null)) setMode(null);
  }, [account.status, mode, removalBlocked]);
  useEffect(() => {
    if (!noticeDismissal.current || busy || account.removalNotice?.id === noticeDismissal.current) return;
    if (!account.removalNotice) menu.current?.focus();
    noticeDismissal.current = null;
  }, [account.removalNotice, busy]);
  return { mode, setMode, draft, setDraft, menu, editor, cancel, noticeDismissal,
    rename: () => { setDraft(account.name ?? account.identity ?? ""); setMode("rename"); } };
}

export function ProviderAccountRow({ account, providerId, providerName, iconSvg, position, count, busy, retryingRemoval, continuation, onAction, onRefresh }: {
  account: Account;
  providerId: string;
  providerName: string;
  iconSvg?: string | null;
  position: number;
  count: number;
  busy: boolean;
  retryingRemoval: boolean;
  continuation: ProviderAccountsSnapshot["continuation"];
  onAction: (action: ProviderAccountsAction) => Promise<boolean>;
  onRefresh: () => void;
}) {
  const removalBlocked = continuation !== "proven" && account.tabsInUse > 0 && account.id !== "default";
  const edit = useRowEditor(account, busy, removalBlocked);
  const identity = account.identity ?? account.name ?? (account.status === "signedOut" ? "Signed out" : account.id === "default" ? "Your usual sign-in" : "Account");
  const name = account.name ?? identity;
  const status = accountStatus(account, retryingRemoval);
  const failedRemoval = account.status === "removing" && account.removalFailureReason != null && !retryingRemoval;
  const removingNow = account.status === "removing" && !failedRemoval && status === "Removing";
  const removalNotice = account.removalNotice;
  const disabled = busy || account.status === "removing";
  const menuDisabled = busy || (account.status === "removing" && !failedRemoval);
  const retryRemoval = () => { void onAction({ action: "remove", accountId: account.id }); };
  const move = (next: number) => {
    if (!disabled && next >= 0 && next < count && next !== position) void onAction({ action: "reorder", accountId: account.id, position: next });
  };
  const items: DropdownMenuEntry[] = [
    { label: "Move to top", disabled: disabled || position === 0, onClick: () => move(0) },
    { label: "Move up", disabled: disabled || position === 0, onClick: () => move(position - 1) },
    { label: "Move down", disabled: disabled || position === count - 1, onClick: () => move(position + 1) },
    { label: "Rename", disabled, onClick: edit.rename },
    { separator: true, key: "remove" },
    { label: "Sign out and remove", disabled: disabled || account.id === "default",
      unavailable: removalBlocked,
      description: account.id === "default" ? `This is your usual sign-in. Sign out from ${providerName} itself.`
        : removalBlocked ? `${account.tabsInUse === 1 ? "1 tab still uses" : `${account.tabsInUse} tabs still use`} it. Close them first.` : undefined,
      onClick: () => edit.setMode("remove") },
  ];
  if (account.status === "probeFailed" && account.probeFailureReason) {
    items.push({ separator: true, key: "probe-failure" },
      { label: "Couldn't check sign-in", description: account.probeFailureReason, unavailable: true });
  }
  if (account.status === "removing") {
    for (const item of items) {
      if ("separator" in item || "items" in item) continue;
      item.disabled = busy;
      item.unavailable = true;
      item.description = "Finishing removal first.";
      item.onClick = undefined;
    }
    if (failedRemoval) items.push({ separator: true, key: "retry-removal" }, { label: "Try again", disabled: busy, onClick: retryRemoval });
  }
  const trimmed = edit.draft.trim();
  const validName = Array.from(trimmed).length > 0 && Array.from(trimmed).length <= 80 && !/\p{Cc}/u.test(trimmed);
  const cancelEdit = () => { if (!busy) edit.setMode(null); };

  return <div role="listitem" tabIndex={0} data-account-id={account.id} aria-label={failedRemoval ? `${identity}, couldn't finish removing` : [name, account.name && account.identity ? account.identity : "", status].filter(Boolean).join(", ")}
    className="ct-row min-w-0 px-2 py-1 focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-border-focus)]"
    onKeyDown={event => {
      if (edit.mode && event.code === "Escape") { event.preventDefault(); event.stopPropagation(); cancelEdit(); return; }
      if (event.target !== event.currentTarget || !event.altKey || edit.mode) return;
      if (event.code === "ArrowUp" || event.code === "ArrowDown") {
        event.preventDefault(); event.stopPropagation(); move(position + (event.code === "ArrowUp" ? -1 : 1));
      }
    }}>
    <div className="flex min-h-11 min-w-0 flex-wrap items-center gap-2">
      <span aria-hidden="true" className="ct-settings-card__icon !h-[26px] !w-[26px] shrink-0 text-xs">{Array.from(name)[0]?.toLocaleUpperCase()}</span>
      {edit.mode === "remove" ? <>
        <p className="min-w-0 flex-1 text-[13px] text-[var(--ct-fg)]">Remove {identity}?{continuation === "proven" && account.tabsInUse > 0 ? " Its tabs continue on the next account." : ""}</p>
        <div className="ml-auto flex items-center">
          <button type="button" className={`${quiet} text-[var(--ct-red)]`} disabled={disabled || removalBlocked} onClick={() => { void onAction({ action: "remove", accountId: account.id }).then(accepted => { if (accepted) edit.setMode(null); }); }}>Remove</button>
          <button ref={edit.cancel} type="button" className={quiet} disabled={busy} onClick={cancelEdit}>Cancel</button>
        </div>
      </> : edit.mode === "rename" ? <form aria-label={`Rename ${identity}`} className="flex min-w-0 flex-1 flex-wrap items-center gap-2"
        onSubmit={event => { event.preventDefault(); if (validName && !disabled) void onAction({ action: "rename", accountId: account.id, name: trimmed }).then(accepted => { if (accepted) edit.setMode(null); }); }}>
        <input ref={edit.editor} aria-label="Account name" value={edit.draft} disabled={disabled} onChange={event => edit.setDraft(event.target.value)}
          className="min-h-11 min-w-0 flex-1 rounded-lg px-3 text-[13px] text-[var(--ct-fg)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-border-focus)]" />
        <button type="submit" className={quiet} disabled={disabled || !validName}>Save</button>
        <button type="button" className={quiet} disabled={busy} onClick={cancelEdit}>Cancel</button>
      </form> : <>
        <div className="min-w-0 flex-[1_1_24ch]">
          <div title={name} className="truncate text-[13px] text-[var(--ct-fg)]">{name}</div>
          {account.name && account.identity && <div title={account.identity} className="truncate text-xs text-[var(--ct-muted-fg)]">{account.identity}</div>}
          {failedRemoval ? <InlineErrorNotice role="status"
            description={{ title: `Couldn't finish removing ${identity}.`, cause: "", actions: [], raw: account.removalFailureReason ?? "" }}
            actions={[{ label: "Try again", disabled: busy, onClick: retryRemoval }]} /> : status && <div className="flex items-center gap-1.5 text-xs text-[var(--ct-muted-fg)]">
            {account.status === "checking" || removingNow ? <DeferredLoader state="present" style={{ animationDelay: "400ms" }}><BusyState inline label={status} /></DeferredLoader>
              : account.status === "signingIn" ? <BusyState inline label={status} /> : <>
              {(account.status === "exhausted" || account.status === "signedOut") && <span aria-hidden="true" className={account.status === "exhausted" ? "h-1.5 w-1.5 shrink-0 rounded-full bg-[var(--ct-yellow)]" : "h-1.5 w-1.5 shrink-0 rounded-full border border-[var(--ct-muted-fg)]"} />}
              <span>{status}</span>
            </>}
          </div>}
        </div>
        <div className="ml-auto flex shrink-0 items-center gap-1">
          {account.status === "signedOut" && <button type="button" className={quiet} disabled={disabled} onClick={() => { void onAction({ action: "signIn", accountId: account.id }); }}>Sign in</button>}
          {account.status === "probeFailed" && <button type="button" className={quiet} disabled={disabled} onClick={() => { void onAction({ action: "check", accountId: account.id }); }}>Check again</button>}
          {account.status === "signingIn" && <button type="button" className={quiet} disabled={disabled} onClick={() => { void onAction({ action: "cancelSignIn", accountId: account.id }); }}>Cancel</button>}
          <DropdownMenu items={items} disabled={menuDisabled} menuLabel={`${identity} actions`}
            trigger={menuDisabled
              ? <button ref={edit.menu} type="button" disabled aria-label={`Actions for ${identity}`} className="ct-row-kebab ct-row-kebab--touch"><KebabGlyph /></button>
              : <RowMenuButton ref={edit.menu} label={`Actions for ${identity}`} touchMode />} />
        </div>
      </>}
    </div>
    {removalNotice?.kind === "inUse" && <InlineErrorNotice
      description={{ title: "Close this account's tabs before removing it.", cause: "", actions: [], raw: "" }}
      actions={[{ label: "Got it", disabled: busy, onClick: () => {
        const noticeId = removalNotice.id;
        edit.noticeDismissal.current = noticeId;
        void onAction({ action: "dismissRemovalNotice", accountId: account.id, noticeId }).then(accepted => { if (!accepted) edit.noticeDismissal.current = null; });
      } }]} />}
    {account.login && account.status === "signingIn" && <RuntimeLoginActions providerId={providerId} tabId={account.login.tabId} state={account.login.state}
      iconSvg={iconSvg} variant="inline" title={`Sign in to ${providerName} as ${identity}`} onCheck={onRefresh} />}
  </div>;
}
