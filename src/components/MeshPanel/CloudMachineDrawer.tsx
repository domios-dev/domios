import { useEffect, useRef, useState } from "react";
import { Moon, Play, X } from "lucide-react";
import {
  cloudMachineCardView,
  cloudSizeOptions,
  formatCloudUptime,
} from "../../lib/cloudMachines/stateModel";
import type {
  CloudPromoRedemption,
  CloudReceipt,
  CloudUsagePayload,
} from "../../lib/cloudMachines/types";
import { BusyState } from "../shared/BusyState";
import { Button } from "../shared/Button";
import { UI_PANEL } from "../shared/design";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { MachineGlyph } from "../shared/MachineGlyph";
import { useEscapeDismissal } from "../shared/useEscapeDismissal";
import { CloudBootstrapPanel } from "../CloudBootstrap/CloudBootstrapPanel";
import { CloudUsageMeter } from "./CloudUsageMeter";
import "./cloudMachines.css";

export interface CloudMachineDrawerProps {
  machine: CloudReceipt;
  usage: CloudUsagePayload | null;
  planId?: string | null;
  idleSleepMinutes: number;
  reachabilityHint?: string;
  busy?: boolean;
  actionError?: string | null;
  onClose: () => void;
  onTopUp: (forWake: boolean) => void;
  onRedeem: (code: string) => Promise<CloudPromoRedemption>;
  onSleep: () => Promise<unknown>;
  onWake: () => Promise<unknown>;
  onDelete: () => Promise<boolean>;
}

export function CloudMachineDrawer({
  machine,
  usage,
  planId,
  idleSleepMinutes,
  reachabilityHint,
  busy = false,
  actionError = null,
  onClose,
  onTopUp,
  onRedeem,
  onSleep,
  onWake,
  onDelete,
}: CloudMachineDrawerProps) {
  useEscapeDismissal(onClose);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const headingRef = useRef<HTMLHeadingElement>(null);

  useEffect(() => {
    headingRef.current?.focus({ preventScroll: true });
  }, []);

  const view = cloudMachineCardView(machine, usage);
  const sizeMeta = cloudSizeOptions(usage, machine.rate_per_hour).find((item) => item.id === machine.size);
  const sizeHeading = sizeMeta?.specsLabel ? `${view.sizeLabel}, ${sizeMeta.specsLabel}` : view.sizeLabel;
  const isAwake = view.state === "awake" || view.state === "draining";
  const isAsleep = view.state === "asleep";
  const isOutOfCredits = view.state === "out_of_credits";
  const isWaking = view.state === "waking" || view.state === "creating" || view.state === "enrolling";

  return (
    <>
      <div className="fixed bottom-0 left-0 right-0 top-9 z-40 bg-black/30" onClick={onClose} />
      <div
        className={UI_PANEL.drawerBase}
        style={{ width: "min(380px, 100vw)", padding: "18px 20px" }}
        role="dialog"
        aria-modal="true"
        aria-labelledby="cloud-machine-drawer-heading"
        data-testid="cloud-machine-drawer"
      >
        <div className="flex items-center gap-2.5">
          <MachineGlyph kind="vm" size="tile" muted={view.tileMuted} />
          <div className="min-w-0 flex-1">
            <h2
              ref={headingRef}
              id="cloud-machine-drawer-heading"
              tabIndex={-1}
              className="m-0 truncate text-[15px] font-semibold text-[var(--ct-fg)] focus:outline-none"
            >
              {machine.name}
            </h2>
            <div className="mt-0.5 flex items-center gap-1.5 text-[11px] text-[var(--ct-muted-fg)]">
              {isWaking ? (
                <BusyState inline hideLabel phase="work" dotSize="micro" label={view.drawerStatusLabel} />
              ) : view.statusIcon === "moon" ? (
                <Moon size={11} className="shrink-0" aria-hidden="true" />
              ) : (
                <span
                  aria-hidden="true"
                  className="inline-block h-1.5 w-1.5 shrink-0 rounded-full"
                  style={{
                    background:
                      view.drawerStatusTone === "ok"
                        ? "var(--ct-green)"
                        : view.drawerStatusTone === "danger"
                          ? "var(--ct-red)"
                          : "var(--ct-muted-fg)",
                  }}
                />
              )}
              <span>{view.drawerStatusLabel}</span>
            </div>
          </div>
          <button
            type="button"
            aria-label="Close"
            title="Close (Esc)"
            onClick={onClose}
            className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md border-0 bg-transparent text-[var(--ct-muted-fg)] cursor-pointer hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)]"
          >
            <X size={16} aria-hidden="true" />
          </button>
        </div>

        <div className="my-1 h-px bg-[var(--ct-border)]" />

        {usage && (
          <>
            <CloudUsageMeter
              usage={usage}
              planId={planId}
              onTopUp={() => onTopUp(isOutOfCredits)}
              onRedeem={onRedeem}
            />
            <div className="my-1 h-px bg-[var(--ct-border)]" />
          </>
        )}

        {isAwake && (
          <>
            <CloudBootstrapPanel machineId={machine.machine_id} />
            <div className="my-1 h-px bg-[var(--ct-border)]" />
          </>
        )}

        <dl className="m-0 grid gap-3 text-[12.5px]">
          <div className="flex items-start gap-2.5">
            <dt className="w-[88px] shrink-0 text-[var(--ct-muted-fg)]">Size</dt>
            <dd className="m-0 min-w-0 flex-1">
              <div className="text-[var(--ct-fg)]">{sizeHeading}</div>
              {view.rateLabel && (
                <div className="mt-0.5 text-[11px] tabular-nums text-[var(--ct-muted-fg)]">
                  {view.rateLabel} while running
                </div>
              )}
            </dd>
          </div>

          <div className="flex items-start gap-2.5">
            <dt className="w-[88px] shrink-0 text-[var(--ct-muted-fg)]">Auto-sleep</dt>
            <dd className="m-0 min-w-0 flex-1">
              <div className="text-[var(--ct-fg)]">After {idleSleepMinutes} min idle</div>
              <div className="mt-0.5 text-[11px] text-[var(--ct-muted-fg)]">Only active time is billed</div>
            </dd>
          </div>

          {machine.uptime_secs > 0 && (
            <div className="flex items-start gap-2.5">
              <dt className="w-[88px] shrink-0 text-[var(--ct-muted-fg)]">Uptime</dt>
              <dd className="m-0 min-w-0 flex-1 tabular-nums text-[var(--ct-fg)]">
                {formatCloudUptime(machine.uptime_secs)}
              </dd>
            </div>
          )}

          {reachabilityHint && (
            <p className="m-0 text-[11.5px] leading-[1.45] text-[var(--ct-muted-fg)]">{reachabilityHint}</p>
          )}
        </dl>

        {actionError && <InlineErrorNotice role="alert" description={{ title: actionError, cause: "", actions: [], raw: "" }} />}

        <div className="mt-auto flex flex-col gap-2.5 pt-3">
          {confirmDelete ? (
            <div
              role="group"
              aria-label={`Delete ${machine.name}`}
              className="flex flex-col gap-2 rounded-[10px] border border-[var(--ct-border)] bg-[color-mix(in_srgb,var(--ct-fg)_2%,var(--ct-bg))] p-3"
            >
              <p className="m-0 text-xs leading-[1.45] text-[var(--ct-fg)]">
                Delete <strong>{machine.name}</strong>? Any unpushed work on this cloud machine will be permanently lost.
              </p>
              <div className="flex items-center justify-end gap-2">
                <Button type="button" size="sm" variant="ghost" disabled={busy} onClick={() => setConfirmDelete(false)}>
                  Cancel
                </Button>
                <Button
                  type="button"
                  size="sm"
                  variant="danger"
                  disabled={busy}
                  loading={busy}
                  onClick={async () => {
                    const deleted = await onDelete();
                    if (deleted) onClose();
                  }}
                >
                  Delete machine
                </Button>
              </div>
            </div>
          ) : (
            <div className="flex items-center justify-between gap-2">
              {isAwake && (
                <Button
                  type="button"
                  size="sm"
                  variant="ghost"
                  disabled={busy}
                  loading={busy}
                  onClick={() => void onSleep()}
                >
                  <Moon size={12} aria-hidden="true" />
                  Sleep now
                </Button>
              )}
              {isAsleep && (
                <Button
                  type="button"
                  size="sm"
                  variant="primary"
                  disabled={busy}
                  loading={busy}
                  onClick={() => void onWake()}
                >
                  <Play size={12} aria-hidden="true" />
                  Wake
                </Button>
              )}
              {isOutOfCredits && (
                <Button type="button" size="sm" variant="primary" onClick={() => onTopUp(true)}>
                  Top up to wake
                </Button>
              )}
              {!isAwake && !isAsleep && !isOutOfCredits && <span />}

              <button
                type="button"
                disabled={busy}
                onClick={() => setConfirmDelete(true)}
                className="border-0 bg-transparent p-0 text-xs text-[var(--ct-red)] cursor-pointer hover:underline disabled:opacity-50"
              >
                Delete machine
              </button>
            </div>
          )}
        </div>
      </div>
    </>
  );
}
