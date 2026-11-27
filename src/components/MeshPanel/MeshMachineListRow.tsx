import { useCallback, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { ExternalLink, Moon, Pencil, Play, SlidersHorizontal } from "lucide-react";
import { cloudHoursStatus } from "../../lib/cloudMachines/usageModel";
import { appVersion } from "../../lib/buildStamp";
import { LOCAL_MACHINE_ID } from "../../lib/machinePlacement";
import { machineLabel, UNKNOWN_MACHINE } from "../../lib/machineIdentity";
import { formatAge } from "../../lib/timeLabels";
import { useMeshMachineIdentity } from "../../lib/useMeshMachineIdentities";
import { LockBadge } from "../billing/LockBadge";
import { lockedDimClass } from "../billing/freeLimitModel";
import { meshMachineLockedView } from "../billing/lockSurfaces";
import { plansAction } from "../billing/purchaseSwitch";
import { BusyState } from "../shared/BusyState";
import { DropdownMenu, type DropdownMenuEntry } from "../shared/DropdownMenu";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { MachineGlyph } from "../shared/MachineGlyph";
import { UI_BUTTON, UI_TEXT } from "../shared/design";
import type { MachineTone } from "./accountCanvas";
import { REAUTH_REQUIRED_COPY, REMOVED_COPY } from "./accountMachines";
import { DuplicateMachineNameHint, sameMachineNameDescription } from "./DuplicateMachineNameHint";
import { MachineIdentityPopover } from "./MachineIdentityEditor";
import { MachineName } from "./MachineName";
import type { CanvasAccount, CanvasCloud } from "./MeshCanvas";
import type { MeshMachineRow } from "./meshMachineListModel";

const TONE_CLASS: Record<MachineTone, string> = {
  ok: "ct-mesh-tone--ok",
  progress: "ct-mesh-tone--progress",
  muted: "ct-mesh-tone--muted",
  warning: "ct-mesh-tone--warning",
  danger: "ct-mesh-tone--danger",
};

export interface MeshMachineListRowProps {
  row: MeshMachineRow;
  now: number;
  selected: boolean;
  narrow: boolean;
  touch: boolean;
  localVersion?: string;
  account?: CanvasAccount;
  cloud?: CanvasCloud;
  onPeerClick: (peerId: string) => void;
  onGoOnline?: () => void;
  onOpenPeerWebUi?: (url: string) => void;
  onMoveFocus: (rowId: string, direction: "next" | "prev" | "first" | "last") => void;
  registerRowElement: (rowId: string, element: HTMLElement | null) => void;
}

export function MeshMachineListRow({
  row,
  now,
  selected,
  narrow,
  touch,
  localVersion = appVersion(),
  account,
  cloud,
  onPeerClick,
  onGoOnline,
  onOpenPeerWebUi,
  onMoveFocus,
  registerRowElement,
}: MeshMachineListRowProps) {
  const identity = useMeshMachineIdentity(
    row.isCloud ? null : row.isThisMachine ? undefined : row.peerId ?? null,
    !row.isCloud && (row.isThisMachine || Boolean(row.peerId)),
    row.isThisMachine ? undefined : row.name,
  );
  const [confirm, setConfirm] = useState<"sign_out" | "remove" | null>(null);
  const [alsoSignOut, setAlsoSignOut] = useState(false);
  const [acting, setActing] = useState(false);
  const [editingIdentity, setEditingIdentity] = useState<"appearance" | "name" | null>(null);
  const menuAnchorRef = useRef<HTMLButtonElement | null>(null);
  const returnFocusRef = useRef(false);

  useLayoutEffect(() => {
    if (editingIdentity || !returnFocusRef.current) return;
    returnFocusRef.current = false;
    menuAnchorRef.current?.focus({ preventScroll: true });
  }, [editingIdentity]);

  const closeIdentity = useCallback(() => {
    returnFocusRef.current = true;
    setEditingIdentity(null);
  }, []);

  const activatePrimary = useCallback(() => {
    if (!row.canOpen) return;
    if (row.isCloud && row.cloudMachine && cloud) {
      cloud.onSelectMachine(row.cloudMachine.machine_id);
      return;
    }
    if (row.isThisMachine) {
      onPeerClick(LOCAL_MACHINE_ID);
      return;
    }
    if (row.peerId) {
      onPeerClick(row.peerId);
    }
  }, [cloud, onPeerClick, row]);

  const openActionMenu = useCallback(() => {
    const trigger = menuAnchorRef.current;
    if (!trigger || trigger.disabled) return;
    trigger.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
  }, []);

  const handleRowKeyDown = useCallback(
    (event: KeyboardEvent<HTMLElement>) => {
      if (event.target !== event.currentTarget) return;
      if (event.key === "ArrowDown") {
        event.preventDefault();
        onMoveFocus(row.id, "next");
        return;
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        onMoveFocus(row.id, "prev");
        return;
      }
      if (event.key === "Home") {
        event.preventDefault();
        onMoveFocus(row.id, "first");
        return;
      }
      if (event.key === "End") {
        event.preventDefault();
        onMoveFocus(row.id, "last");
        return;
      }
      if (event.key === "Enter") {
        if (!row.canOpen) return;
        event.preventDefault();
        activatePrimary();
        return;
      }
      if (event.key === "ContextMenu" || (event.shiftKey && event.key === "F10")) {
        event.preventDefault();
        openActionMenu();
      }
    },
    [activatePrimary, onMoveFocus, openActionMenu, row.canOpen, row.id],
  );

  const lock = row.accountRow ? meshMachineLockedView(row.accountRow) : null;
  const accountBusy = Boolean(row.accountKey && account?.busy === row.accountKey);
  const cloudBusy = Boolean(row.cloudMachine && cloud?.busyMachineId === row.cloudMachine.machine_id);
  const busy = accountBusy || cloudBusy || acting;

  const renameSave = row.isThisMachine
    ? account?.onRenameSelf
    : row.accountRow && row.canRename && account?.actions.onRename
      ? (next: string) => account.actions.onRename!(row.accountRow!, next)
      : undefined;

  const renameControl = renameSave ? { displayName: row.displayName, save: renameSave } : undefined;
  const identityPeer = row.isCloud
    ? undefined
    : row.isThisMachine
      ? null
      : row.canRename && row.peerId
        ? row.peerId
        : undefined;

  const submitSessionAction = async () => {
    if (!row.accountRow || !account) return;
    setActing(true);
    try {
      const succeeded =
        confirm === "remove" && row.revokeTarget && account.actions.onRemove
          ? await account.actions.onRemove(row.accountRow, row.revokeTarget, row.online || alsoSignOut)
          : account.actions.onSignOut
            ? await account.actions.onSignOut(row.accountRow)
            : false;
      if (succeeded) {
        setConfirm(null);
        setAlsoSignOut(false);
      }
    } finally {
      setActing(false);
    }
  };

  const hoursStatus = row.isCloud && row.cloudMachine
    ? cloudHoursStatus(row.cloudUsage, row.cloudState === "out_of_credits")
    : null;
  const archiving = row.storageTier === "evicting";

  const menuItems: DropdownMenuEntry[] = [];
  if (row.canOpen) {
    menuItems.push({
      label: row.isCloud ? "Open details" : row.isThisMachine ? "Open details" : "Switch to",
      icon: <SlidersHorizontal size={13} aria-hidden="true" />,
      onClick: activatePrimary,
    });
  }
  if (row.isCloud && row.cloudMachine && cloud) {
    const cm = row.cloudMachine;
    if ((row.cloudState === "awake" || row.cloudState === "draining") && cloud.onSleepMachine) {
      menuItems.push({
        label: "Sleep",
        icon: <Moon size={13} aria-hidden="true" />,
        disabled: busy,
        onClick: () => { void cloud.onSleepMachine!(cm.machine_id); },
      });
    }
    if (row.cloudState === "asleep" && !archiving && cloud.onWakeMachine) {
      menuItems.push({
        label: "Wake",
        icon: <Play size={13} aria-hidden="true" />,
        disabled: busy,
        onClick: () => { void cloud.onWakeMachine!(cm.machine_id); },
      });
    }
    if (hoursStatus) {
      menuItems.push({
        label: "Top up",
        onClick: () => cloud.onTopUpMachine(cm),
      });
    }
  }
  if (row.webUrl && onOpenPeerWebUi && !row.isThisMachine) {
    menuItems.push({
      label: "Open Web UI",
      icon: <ExternalLink size={13} aria-hidden="true" />,
      onClick: () => onOpenPeerWebUi(row.webUrl!),
    });
  }
  if (identityPeer !== undefined) {
    menuItems.push({
      label: "Change icon",
      onClick: () => setEditingIdentity("appearance"),
    });
  }
  if (renameControl || row.sameNameOthers) {
    menuItems.push({
      label: "Rename…",
      icon: <Pencil size={13} aria-hidden="true" />,
      description: row.sameNameOthers ? sameMachineNameDescription(row.sameNameOthers) : undefined,
      disabled: !renameControl,
      onClick: () => setEditingIdentity("name"),
    });
  }
  if (row.plansAction && row.accountKey && account) {
    menuItems.push({
      label: plansAction("See plans"),
      onClick: () => account.actions.onPlans(row.accountKey!),
    });
  }
  if (row.canSignOut && row.accountRow && account?.actions.onSignOut) {
    if (menuItems.length > 0) menuItems.push({ separator: true, key: "session" });
    menuItems.push({
      label: "Sign out this machine",
      onClick: () => setConfirm("sign_out"),
    });
  }
  if (row.revokeTarget && row.accountRow && account?.actions.onRemove) {
    if (menuItems.length > 0 && !row.canSignOut) menuItems.push({ separator: true, key: "remove" });
    menuItems.push({
      label: "Remove from mesh",
      danger: true,
      onClick: () => setConfirm("remove"),
    });
  }

  const seenAge = row.showLastSeen && row.lastSeenMs !== null ? formatAge(row.lastSeenMs, now) : null;
  const versionMismatch = Boolean(row.version && row.version !== localVersion);
  const glyphMuted = !(row.tone === "ok" || row.tone === "progress");
  const rowAriaLabel = row.isCloud
    ? `${row.name}, cloud machine, ${row.statusLabel}${hoursStatus ? `, ${hoursStatus}` : ""}`
    : machineLabel(row.name, identity, row.statusLabel);

  const toneClass = TONE_CLASS[row.tone];
  const statusIndicator = row.connecting ? (
    <BusyState inline hideLabel label={row.statusLabel} phase="work" dotSize="micro" />
  ) : row.statusIcon === "moon" ? (
    <Moon size={12} className="shrink-0" aria-hidden="true" />
  ) : (
    <span aria-hidden="true" className={`ct-mesh-dot ${toneClass}`} />
  );

  const primaryAction = (() => {
    if (row.isThisMachine && row.meshOff && onGoOnline) {
      return { label: "Go online", tone: "accent" as const, hoverOnly: false, onClick: onGoOnline };
    }
    if (row.isThisMachine && row.removed && account?.onRejoin) {
      return { label: account.signingIn ? "Signing in…" : "Sign in", tone: "accent" as const, hoverOnly: false, onClick: account.onRejoin };
    }
    if (row.isThisMachine && row.needsReauth && !row.removed && account) {
      return { label: account.signingIn ? "Signing in…" : "Sign in", tone: "accent" as const, hoverOnly: false, onClick: account.onSignIn };
    }
    if (row.signedOutRow && account) {
      return { label: "Sign in", tone: "accent" as const, hoverOnly: false, onClick: account.onSignIn };
    }
    if (row.plansAction && row.accountKey && account) {
      return { label: plansAction("See plans"), tone: "ghost" as const, hoverOnly: false, onClick: () => account.actions.onPlans(row.accountKey!) };
    }
    if (row.isCloud && row.cloudMachine && cloud) {
      const cm = row.cloudMachine;
      if (hoursStatus) {
        return { label: "Top up", tone: "accent" as const, hoverOnly: false, onClick: () => cloud.onTopUpMachine(cm) };
      }
      if ((row.cloudState === "awake" || row.cloudState === "draining") && cloud.onSleepMachine) {
        return { label: "Sleep", tone: "ghost" as const, hoverOnly: false, onClick: () => { void cloud.onSleepMachine!(cm.machine_id); } };
      }
      if (row.cloudState === "asleep" && !archiving && cloud.onWakeMachine) {
        return { label: "Wake", tone: "ghost" as const, hoverOnly: false, onClick: () => { void cloud.onWakeMachine!(cm.machine_id); } };
      }
    }
    if (row.kind === "peer" && row.online && row.canOpen && !narrow) {
      return { label: "Switch to", tone: "ghost" as const, hoverOnly: true, onClick: activatePrimary };
    }
    return null;
  })();

  const phoneFact = seenAge ?? (row.isCloud ? row.cloudSizeSpecs : row.version ? `Domios ${row.version}` : row.detailLine);

  const desktopDetail = row.removed
    ? REMOVED_COPY
    : row.needsReauth
      ? REAUTH_REQUIRED_COPY
      : row.detailLine;

  return (
    <div role="listitem" className="ct-mesh-list-item">
      <div
        ref={(el) => registerRowElement(row.id, el)}
        tabIndex={0}
        role={row.canOpen ? "button" : "group"}
        aria-label={rowAriaLabel}
        aria-disabled={row.kind === "peer" && !row.canOpen ? "true" : undefined}
        data-testid={`mesh-list-row-${row.id}`}
        data-mesh-row-id={row.id}
        data-mesh-group={row.group}
        data-selected={selected ? "true" : undefined}
        onClick={row.canOpen ? activatePrimary : undefined}
        onKeyDown={handleRowKeyDown}
        className={[
          "ct-mesh-row",
          touch ? "ct-mesh-row--touch" : "",
          row.canOpen ? "cursor-pointer" : "cursor-default",
          selected ? "ct-mesh-row--selected" : "",
        ]
          .filter(Boolean)
          .join(" ")}
      >
        <MachineGlyph
          {...(row.isCloud ? { kind: "vm", muted: glyphMuted } : { ...identity, muted: glyphMuted })}
          size="tile"
        />

        <div className="ct-mesh-row__tx">
          <div className="ct-mesh-row__name">
            <span className={`min-w-0 truncate ${lock ? lockedDimClass(lock) : ""}`}>
              <MachineName
                name={row.name}
                displayName={row.displayName}
                onRename={renameSave}
              />
            </span>
            {row.isThisMachine && <span className="ct-mesh-you">You</span>}
            {lock && <LockBadge view={lock} />}
            {hoursStatus && <span className="ct-mesh-tag ct-mesh-tone--warning">{hoursStatus}</span>}
          </div>

          {narrow ? (
            <div className="ct-mesh-row__sub">
              {statusIndicator}
              <span className="text-[var(--ct-fg)]">{row.statusLabel}</span>
              {phoneFact && <span className="truncate">· {phoneFact}</span>}
            </div>
          ) : (
            <div className="ct-mesh-row__sub">
              {desktopDetail && <span className="truncate">{desktopDetail}</span>}
              {row.version && (
                <>
                  {desktopDetail && <span aria-hidden="true">·</span>}
                  <span
                    className="inline-flex shrink-0 items-center gap-1 whitespace-nowrap tabular-nums"
                    title={row.build ? `Build ${row.build}` : undefined}
                  >
                    {versionMismatch && (
                      <span
                        role="img"
                        aria-label={`This machine runs ${localVersion}`}
                        title={`This machine runs ${localVersion}`}
                        className="ct-mesh-dot ct-mesh-tone--warning"
                      />
                    )}
                    Domios {row.version}
                  </span>
                </>
              )}
              {seenAge && (
                <>
                  {(desktopDetail || row.version) && <span aria-hidden="true">·</span>}
                  <span className="shrink-0 whitespace-nowrap tabular-nums">last seen {seenAge}</span>
                </>
              )}
            </div>
          )}
        </div>

        {!narrow && (
          <span
            data-testid="mesh-row-status"
            className={`ct-mesh-tag ${toneClass}`}
          >
            {statusIndicator}
            <span>{row.statusLabel}</span>
          </span>
        )}

        <div
          className="ct-mesh-row__actions"
          onClick={(event) => event.stopPropagation()}
          onKeyDown={(event) => event.stopPropagation()}
        >
          {primaryAction && (
            <button
              type="button"
              disabled={busy}
              onClick={primaryAction.onClick}
              className={[
                "ct-mesh-act-btn",
                primaryAction.tone === "accent" ? "ct-mesh-act-btn--accent" : "ct-mesh-act-btn--ghost",
                primaryAction.hoverOnly ? "ct-mesh-act-btn--hover" : "",
                touch ? "ct-mesh-act-btn--touch" : "",
              ]
                .filter(Boolean)
                .join(" ")}
            >
              {primaryAction.label}
            </button>
          )}

          {menuItems.length > 0 && (
            <span className="inline-flex">
              <DropdownMenu
                placement="bottom-end"
                disabled={busy}
                menuLabel={`Actions for ${row.name}`}
                items={menuItems}
                trigger={
                  <button
                    ref={menuAnchorRef}
                    type="button"
                    disabled={busy}
                    aria-label={`More for ${row.name}`}
                    className={`ct-mesh-more${touch ? " ct-mesh-more--touch" : ""}`}
                  >
                    ⋯
                  </button>
                }
              />
              {editingIdentity && (
                <MachineIdentityPopover
                  name={row.name}
                  peerId={identityPeer ?? undefined}
                  rename={renameControl}
                  focusName={editingIdentity === "name"}
                  initial={identity ?? UNKNOWN_MACHINE}
                  onClose={closeIdentity}
                  anchor={menuAnchorRef}
                />
              )}
            </span>
          )}
        </div>
      </div>

      {row.sameNameOthers ? (
        <div className="ct-mesh-row__extra" onClick={(event) => event.stopPropagation()}>
          <DuplicateMachineNameHint
            name={row.name}
            others={row.sameNameOthers}
            peerId={row.peerId ?? undefined}
            rename={renameControl}
          />
        </div>
      ) : null}

      {row.callout && row.accountKey && account && (
        <div className="ct-mesh-row__extra" onClick={(event) => event.stopPropagation()}>
          {row.callout.transient ? (
            <p role="status" className={UI_TEXT.metaMuted}>{row.callout.message}</p>
          ) : (
            <InlineErrorNotice
              role="alert"
              description={{ title: row.callout.title, cause: row.callout.message, raw: row.callout.message, actions: [] }}
              actions={[
                {
                  label: "Dismiss",
                  ariaLabel: `Dismiss: ${row.callout.title}`,
                  onClick: () => account.actions.onDismiss(row.accountKey!),
                },
              ]}
            />
          )}
        </div>
      )}

      {confirm && (
        <div
          role="group"
          aria-label={`${confirm === "remove" ? "Remove" : "Sign out"} ${row.name}`}
          className="ct-mesh-row__confirm"
          onClick={(event) => event.stopPropagation()}
          onKeyDown={(event) => event.stopPropagation()}
        >
          <p className={`${UI_TEXT.meta} m-0`}>
            {confirm === "remove"
              ? row.online
                ? `Remove ${row.name} from mesh? It will be signed out first.`
                : `Remove ${row.name} from mesh?`
              : `Sign out ${row.name}? It will need to sign in again to use your account.`}
          </p>
          {confirm === "remove" && !row.online && row.canSignOut && (
            <label className={`${UI_TEXT.meta} flex items-center gap-1.5`}>
              <input
                type="checkbox"
                checked={alsoSignOut}
                disabled={busy}
                onChange={(event) => setAlsoSignOut(event.target.checked)}
              />
              Also sign it out
            </label>
          )}
          <div className="flex items-center gap-2">
            <button
              type="button"
              disabled={busy}
              aria-busy={busy}
              className={`${UI_BUTTON.danger} ${UI_BUTTON.compact}`}
              onClick={() => void submitSessionAction()}
            >
              {confirm === "remove" ? "Confirm remove" : "Confirm sign out"}
            </button>
            <button
              type="button"
              disabled={busy}
              className={`${UI_BUTTON.ghost} ${UI_BUTTON.compact}`}
              onClick={() => {
                setConfirm(null);
                setAlsoSignOut(false);
              }}
            >
              Cancel
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
