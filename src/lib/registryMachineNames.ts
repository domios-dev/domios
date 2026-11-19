import { cleanMachineName } from "./machineIdentity";

interface RegistryName {
  installation_id: string;
  admitted_at: number;
  display_name: string | null;
  active: boolean;
}

export interface DuplicateMachineName {
  name: string;
  installations: string[];
}

export function registryMachineNames(rows: readonly RegistryName[]) {
  const labels = new Map<string, string>();
  const groups = new Map<string, RegistryName[]>();
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
