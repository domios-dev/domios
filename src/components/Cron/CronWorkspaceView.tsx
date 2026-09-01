import { useEffect, useState } from "react";
import { Clock3, X } from "lucide-react";
import { invoke, listen } from "../../lib/bridge";
import type { CronJob } from "../../lib/types";
import { CronDrawerSection } from "../MeshPanel/CronDrawerSection";
import type { MeshResourceActionAdapter } from "../MeshPanel/meshResourceActions";
import { CRON_ID_PREFIX, type Service } from "../MeshPanel/types";
import type { MeshInventoryView } from "../MeshPanel/servicesView";
import { cronSelectionStore } from "./cronSelectionStore";

interface Props {
  cronId: string;
  actionAdapter?: MeshResourceActionAdapter;
}

// Same inventory projection MeshPanel renders from — never a hand-built capability map.
export function selectCronService(inventory: MeshInventoryView, cronId: string): Service | null {
  const resourceId = `${CRON_ID_PREFIX}${cronId}`;
  return inventory.resources.find((resource) => resource.id === resourceId) ?? null;
}

export function CronWorkspaceView({ cronId, actionAdapter }: Props) {
  const [job, setJob] = useState<CronJob | null>(null);
  const [service, setService] = useState<Service | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = async () => {
    try {
      const [nextJob, inventory] = await Promise.all([
        invoke<CronJob>("cron_get", { id: cronId }),
        invoke<MeshInventoryView>("mesh_inventory_list"),
      ]);
      setJob(nextJob);
      setService(selectCronService(inventory, cronId));
      setError(null);
    } catch (cause) {
      setError(String(cause));
    }
  };

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void refresh();
    void listen("cron-run-finished", () => {
      if (active) void refresh();
    }).then((stop) => {
      if (active) unlisten = stop;
      else stop();
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [cronId]);

  if (error) {
    return <div className="h-full grid place-items-center text-sm text-[var(--ct-red)]">{error}</div>;
  }
  if (!job || !service) return null;

  const target = job.report_target_tab_id
    ? `Reports to Tab ${job.report_target_tab_id}`
    : "Runs in a private cron worker";

  return (
    <section className="h-full overflow-y-auto bg-[var(--ct-bg)]" aria-label={`Cron ${job.name}`}>
      <header className="sticky top-0 z-10 flex items-center gap-3 border-b border-[var(--ct-border)] bg-[var(--ct-titlebar-bg)] px-5 py-3">
        <Clock3 size={18} className="text-[var(--ct-accent)]" aria-hidden="true" />
        <div className="min-w-0 flex-1">
          <h2 className="truncate text-sm font-semibold text-[var(--ct-fg)]">{job.name}</h2>
          <p className="truncate text-[10px] text-[var(--ct-muted)]">{target}</p>
        </div>
        <button type="button" onClick={() => cronSelectionStore.close()} aria-label="Close cron details" className="p-1 text-[var(--ct-muted)] hover:text-[var(--ct-fg)]">
          <X size={16} />
        </button>
      </header>
      <div className="mx-auto w-full max-w-3xl p-5">
        <CronDrawerSection service={service} machineName="This machine" onUpdate={() => void refresh()} actionAdapter={actionAdapter} />
      </div>
    </section>
  );
}
