import { useCallback, useMemo, useRef, useState, type ReactNode } from "react";
import { ChevronDown, ChevronRight, Globe, Search } from "lucide-react";
import { CanvasToolbar } from "@codeterm/chat-ui/CanvasToolbar";
import { formatCreditHours } from "../../lib/cloudMachines/stateModel";
import { useMeshMachineIdentity } from "../../lib/useMeshMachineIdentities";
import { MachineGlyph } from "../shared/MachineGlyph";
import { AccountStateCard } from "./AccountCanvasNodes";
import { CloudMachineCard } from "./CloudMachineCard";
import { CloudMachineSlot } from "./CloudMachineSlot";
import type { CanvasAccount, CanvasCloud } from "./MeshCanvas";
import { MeshMachineListRow } from "./MeshMachineListRow";
import { StaleCleanupConfirm } from "./StaleCleanupConfirm";
import { selectStaleMachineRows, staleCleanupActionLabel } from "./staleMachinesModel";
import {
  collapsedGroupSummary,
  FILTER_VISIBLE_THRESHOLD,
  filterMeshMachineRows,
  groupMeshMachineRows,
  isGroupCollapsed,
  projectMeshMachineRows,
  type MeshMachineGroupId,
  type MeshMachineRow,
} from "./meshMachineListModel";
import type { MeshCollapsedGroupOverrides } from "./meshViewModeModel";
import type { CanvasTailscale } from "./TailscaleNodes";
import type { PeerStatus } from "./useMeshData";
import "./meshMachineList.css";

const LIST_QR_SIZE = 160;

function CollapsedFace({ row }: { row: MeshMachineRow }) {
  const identity = useMeshMachineIdentity(
    row.isCloud ? null : row.peerId ?? null,
    !row.isCloud && Boolean(row.peerId),
    row.name,
  );
  return (
    <span className="ct-mesh-fold__face">
      <MachineGlyph {...(row.isCloud ? { kind: "vm", muted: true } : { ...identity, muted: true })} size="mark-12" />
    </span>
  );
}

export interface MeshMachineListProps {
  toolbarView: ReactNode;
  webCard?: (qrSize: number) => ReactNode;
  webSelected?: boolean;
  webStatus?: string;
  onWebClick?: () => void;
  thisMachine: { name: string; ip: string };
  peers: PeerStatus[];
  account?: CanvasAccount;
  cloud?: CanvasCloud;
  tailscale?: CanvasTailscale;
  meshEnabled?: boolean;
  width: number;
  height: number;
  touch?: boolean;
  selectedId?: string | null;
  collapsedOverrides: MeshCollapsedGroupOverrides;
  onToggleGroup: (groupId: MeshMachineGroupId, rowCount: number) => void;
  onPeerClick: (peerId: string) => void;
  onRefresh?: () => void;
  onGoOnline?: () => void;
  onOpenPeerWebUi?: (url: string) => void;
  webUiBaseUrl?: string | null;
  webUiToken?: string | null;
}

export function MeshMachineList({
  toolbarView,
  webCard,
  webSelected = false,
  webStatus = "Unavailable",
  onWebClick,
  thisMachine,
  peers,
  account,
  cloud,
  tailscale,
  meshEnabled = true,
  width,
  height,
  touch = false,
  selectedId = null,
  collapsedOverrides,
  onToggleGroup,
  onPeerClick,
  onRefresh,
  onGoOnline,
  onOpenPeerWebUi,
  webUiBaseUrl = null,
  webUiToken = null,
}: MeshMachineListProps) {
  const [filterQuery, setFilterQuery] = useState("");
  const [cleanupOpen, setCleanupOpen] = useState(false);
  const [cleanupBusy, setCleanupBusy] = useState(false);
  const [cleanupError, setCleanupError] = useState<string | null>(null);
  const accountNodes = account?.canvas.nodes;
  const staleMachines = useMemo(
    () => selectStaleMachineRows((accountNodes ?? []).map((node) => node.row)),
    [accountNodes],
  );
  const onRemoveStale = account?.actions.onRemoveStale;
  const confirmCleanup = useCallback(async () => {
    if (!onRemoveStale) return;
    setCleanupBusy(true);
    setCleanupError(null);
    const result = await onRemoveStale(staleMachines.map((machine) => machine.installationId));
    setCleanupBusy(false);
    if (result) setCleanupOpen(false);
    else setCleanupError("Some machines could not be removed. Try again.");
  }, [onRemoveStale, staleMachines]);
  const rowElementsRef = useRef<Map<string, HTMLElement>>(new Map());
  const now = account?.now ?? Date.now();
  const narrow = width > 0 && width < 600;
  const compactSide = width > 0 && width < 760;
  const showCloudSlot = Boolean(cloud && (cloud.pendingCreate || cloud.slotOpen));

  const allRows = useMemo(
    () =>
      projectMeshMachineRows({
        thisMachine,
        peers,
        account,
        cloudMachines: cloud?.machines,
        cloudUsage: cloud?.usage,
        meshEnabled,
        tailscaleState: tailscale?.state,
        webUiBaseUrl,
        webUiToken,
      }),
    [
      thisMachine,
      peers,
      account,
      cloud?.machines,
      cloud?.usage,
      meshEnabled,
      tailscale?.state,
      webUiBaseUrl,
      webUiToken,
    ],
  );

  const showFilter = allRows.length > FILTER_VISIBLE_THRESHOLD;
  const filteredRows = useMemo(
    () => (showFilter ? filterMeshMachineRows(allRows, filterQuery) : allRows),
    [allRows, filterQuery, showFilter],
  );
  const groups = useMemo(() => groupMeshMachineRows(filteredRows), [filteredRows]);

  const visibleRowIds = useMemo(() => {
    const ids: string[] = [];
    for (const group of groups) {
      if (isGroupCollapsed(group.id, group.rows.length, collapsedOverrides)) continue;
      for (const row of group.rows) ids.push(row.id);
    }
    return ids;
  }, [groups, collapsedOverrides]);

  const registerRowElement = useCallback((rowId: string, element: HTMLElement | null) => {
    if (element) rowElementsRef.current.set(rowId, element);
    else rowElementsRef.current.delete(rowId);
  }, []);

  const handleMoveFocus = useCallback(
    (currentId: string, direction: "next" | "prev" | "first" | "last") => {
      if (visibleRowIds.length === 0) return;
      const index = visibleRowIds.indexOf(currentId);
      let nextIndex = index;
      if (direction === "first") nextIndex = 0;
      else if (direction === "last") nextIndex = visibleRowIds.length - 1;
      else if (direction === "next") nextIndex = Math.min(visibleRowIds.length - 1, index + 1);
      else nextIndex = Math.max(0, index - 1);
      const targetId = visibleRowIds[nextIndex];
      const target = targetId ? rowElementsRef.current.get(targetId) : undefined;
      target?.focus();
    },
    [visibleRowIds],
  );

  const accountCard =
    (cloud?.machines.filter((m) => m.state !== "deleted").length ?? 0) > 0 || showCloudSlot
      ? null
      : (account?.canvas.card ?? null);

  const cloudHoursLabel =
    cloud?.usage && Number.isFinite(cloud.usage.remaining)
      ? `${formatCreditHours(cloud.usage.remaining)} left this month`
      : null;

  const filterControl = showFilter ? (
    <label className="ct-mesh-filter">
      <Search size={13} aria-hidden="true" className="shrink-0 text-[var(--ct-muted-fg)]" />
      <input
        type="search"
        value={filterQuery}
        onChange={(event) => setFilterQuery(event.target.value)}
        placeholder="Filter machines"
        aria-label="Filter machines"
        data-testid="mesh-list-filter"
        className="min-w-0 flex-1 border-0 bg-transparent text-[12px] text-[var(--ct-fg)] placeholder:text-[var(--ct-muted-fg)] focus:outline-none"
      />
    </label>
  ) : undefined;

  return (
    <div
      data-testid="mesh-machine-list-view"
      className="ct-mesh-list-shell flex h-full w-full flex-col overflow-hidden bg-[var(--ct-bg)]"
      style={{ minHeight: height }}
    >
      <div className="ct-mesh-dock shrink-0">
        <CanvasToolbar
          floating={false}
          label="Mesh inventory controls"
          view={toolbarView}
          extras={filterControl}
          onRefresh={onRefresh}
        />
      </div>

      <div
        data-testid="mesh-inventory-list"
        className={`ct-mesh-list-scroll min-h-0 flex-1 overflow-y-auto overscroll-contain${compactSide ? " ct-mesh-list-scroll--compact" : ""}`}
      >
        <div className="ct-mesh-list-col">
          {account && account.canvas.notes.length > 0 && (
            <div role="status" className="ct-mesh-notes">
              {account.canvas.notes.map((note) => (
                <p key={note} className="m-0">{note}</p>
              ))}
            </div>
          )}

          {cloud?.pendingCreate && (
            <div className="mb-3 flex justify-center">
              <CloudMachineCard
                machine={{
                  machine_id: "pending",
                  name: "Cloud machine",
                  size: cloud.pendingCreate.size,
                  state: cloud.pendingCreate.status === "failed" ? "failed" : "creating",
                }}
                usage={cloud.usage}
                failure={cloud.pendingCreate.failure ?? null}
                onFailureAction={cloud.onFailureAction}
              />
            </div>
          )}

          {!cloud?.pendingCreate && cloud?.slotOpen && (
            <div className="mb-3 flex justify-center">
              <CloudMachineSlot
                usage={cloud.usage}
                isFree={cloud.isFree}
                onCreateCloud={cloud.onCreateCloud}
                onPairComputer={cloud.onPairComputer}
                onCancel={cloud.onCloseSlot}
              />
            </div>
          )}

          {groups.map((group) => {
            const collapsed = isGroupCollapsed(group.id, group.rows.length, collapsedOverrides);
            const listId = `mesh-group-list-${group.id}`;
            const headingId = `mesh-group-heading-${group.id}`;
            const isAttention = group.id === "needs_attention";
            return (
              <section
                key={group.id}
                aria-labelledby={headingId}
                data-testid={`mesh-group-${group.id}`}
                className="ct-mesh-group"
              >
                <button
                  type="button"
                  aria-expanded={!collapsed}
                  aria-controls={listId}
                  data-testid={`mesh-group-toggle-${group.id}`}
                  onClick={() => onToggleGroup(group.id, group.rows.length)}
                  className={`ct-mesh-group__head${isAttention ? " ct-mesh-group__head--attn" : ""}`}
                >
                  {collapsed ? (
                    <ChevronRight size={12} className="shrink-0" aria-hidden="true" />
                  ) : (
                    <ChevronDown size={12} className="shrink-0" aria-hidden="true" />
                  )}
                  <span id={headingId} className="ct-mesh-group__label">
                    {group.label}
                  </span>
                  <span className="ct-mesh-group__count">{group.rows.length}</span>
                  {group.id === "cloud" && cloudHoursLabel && (
                    <span className="ct-mesh-group__aside">{cloudHoursLabel}</span>
                  )}
                </button>
                {isAttention && onRemoveStale && staleMachines.length > 0 && !cleanupOpen && (
                  <button
                    type="button"
                    data-testid="mesh-stale-cleanup"
                    onClick={() => {
                      setCleanupError(null);
                      setCleanupOpen(true);
                    }}
                    className="ct-mesh-fold__show"
                  >
                    {staleCleanupActionLabel(staleMachines.length)}
                  </button>
                )}
                {isAttention && cleanupOpen && (
                  <StaleCleanupConfirm
                    machines={staleMachines}
                    busy={cleanupBusy}
                    error={cleanupError}
                    onConfirm={confirmCleanup}
                    onCancel={() => setCleanupOpen(false)}
                  />
                )}

                {collapsed ? (
                  <div
                    id={listId}
                    className={`ct-card ct-card--static ct-mesh-group__card${isAttention ? " ct-mesh-group__card--attn" : ""}`}
                  >
                    <div className="ct-mesh-fold">
                      <span className="ct-mesh-fold__faces" aria-hidden="true">
                        {group.rows.slice(0, 4).map((item) => (
                          <CollapsedFace key={item.id} row={item} />
                        ))}
                      </span>
                      <span className="min-w-0 flex-1 truncate">
                        {collapsedGroupSummary(group.rows, narrow ? 2 : 4)}
                      </span>
                      <button
                        type="button"
                        onClick={() => onToggleGroup(group.id, group.rows.length)}
                        className="ct-mesh-fold__show"
                      >
                        Show
                      </button>
                    </div>
                  </div>
                ) : (
                  <div
                    id={listId}
                    role="list"
                    aria-labelledby={headingId}
                    className={`ct-card ct-card--static ct-mesh-group__card${isAttention ? " ct-mesh-group__card--attn" : ""}`}
                  >
                    {group.rows.map((row) => {
                      const isSelected =
                        selectedId !== null &&
                        (row.id === selectedId ||
                          (row.isThisMachine && selectedId === "__local_machine__") ||
                          (row.isCloud && row.cloudMachine && selectedId === `cloud:${row.cloudMachine.machine_id}`));
                      return (
                        <MeshMachineListRow
                          key={row.id}
                          row={row}
                          now={now}
                          selected={Boolean(isSelected)}
                          narrow={narrow}
                          touch={touch}
                          account={account}
                          cloud={cloud}
                          onPeerClick={onPeerClick}
                          onGoOnline={onGoOnline}
                          onOpenPeerWebUi={onOpenPeerWebUi}
                          onMoveFocus={handleMoveFocus}
                          registerRowElement={registerRowElement}
                        />
                      );
                    })}
                  </div>
                )}
              </section>
            );
          })}

          {accountCard && account && (
            <div className="mt-4 flex justify-center">
              <AccountStateCard
                card={accountCard}
                signingIn={account.signingIn}
                onSignIn={account.onSignIn}
                onCancel={account.cancelSignIn}
                onRetry={account.onRetry}
              />
            </div>
          )}

          {account && (account.moreMachines || account.onRetryEnrollment) && (
            <div className="mt-4 flex flex-wrap items-center gap-2">
              {account.moreMachines && (
                <button type="button" onClick={account.onRetry} className="ct-btn ct-btn-compact">
                  Load more machines
                </button>
              )}
              {account.onRetryEnrollment && (
                <button
                  type="button"
                  disabled={account.busy !== null}
                  aria-busy={account.busy === "enrollment"}
                  onClick={account.onRetryEnrollment}
                  className="ct-btn ct-btn-compact"
                >
                  Retry enrollment
                </button>
              )}
            </div>
          )}

          <div className="ct-mesh-list-web" data-peer-card>
            {webCard ? (
              webCard(LIST_QR_SIZE)
            ) : (
              <button
                type="button"
                aria-label="Web"
                aria-pressed={webSelected}
                onClick={onWebClick}
                className="w-full rounded-xl border border-[var(--ct-accent)] bg-[var(--ct-bg)] p-4"
              >
                <Globe size={24} aria-hidden="true" />
                <strong>Web</strong>
                <span>{webStatus}</span>
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
