import type { AccountMeshSnapshot } from "../components/MeshPanel/accountMeshClient";
import { cleanMachineName } from "./machineIdentity";

export interface RegistryMachineName {
  installation_id: string;
  admitted_at: number;
  display_name: string | null;
  active: boolean;
}

export interface DuplicateMachineName {
  name: string;
  installations: string[];
}

export function registryMachineNames(rows: readonly RegistryMachineName[]) {
  const labels = new Map<string, string>();
  const groups = new Map<string, RegistryMachineName[]>();
  for (const row of new Map(rows.map(row => [row.installation_id, row])).values()) {
    const name = cleanMachineName(row.display_name, row.installation_id);
    if (!row.active || !name) continue;
    labels.set(row.installation_id, name);
    const known = groups.get(name);
    if (known) known.push(row);
    else groups.set(name, [row]);
  }
  const duplicates: DuplicateMachineName[] = [];
  for (const [name, members] of groups) {
    if (members.length < 2) continue;
    members.sort((a, b) => a.admitted_at - b.admitted_at ||
      (a.installation_id < b.installation_id ? -1 : 1));
    const installations = members.map((row, index) => {
      labels.set(row.installation_id, `${name} #${index + 1}`);
      return row.installation_id;
    });
    duplicates.push({ name, installations });
  }
  return { labels, duplicates };
}

export function accountMachineNameProjection(account: AccountMeshSnapshot | null) {
  const { labels } = registryMachineNames(account?.registry_names ?? []);
  const local = account ? labels.get(account.local_installation_id) : undefined;
  const peers = new Map((account?.pairs ?? []).flatMap(pair => {
    const label = labels.get(pair.installation_id);
    return label ? [[pair.instance_id, label] as const] : [];
  }));
  return { local, peers };
}
