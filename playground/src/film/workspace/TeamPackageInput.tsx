import { useRef, useState } from "react";
import { FileUp } from "lucide-react";
import { Button } from "../../../../src/components/shared/Button";

export interface SampleTeamRole { name: string; kind: "manager" | "worker"; title: string }
export interface SampleTeamPackage { title: string; roles: SampleTeamRole[] }

const MAX_BYTES = 256 * 1024;
const SAMPLE = `name: demo/release-review
version: 1.0.0
title: Release review
description: Review Fieldnote releases against their test evidence.
roles:
  orchestrator:
    kind: manager
    instructions: Review the release evidence.
  planner:
    kind: manager
    instructions: Plan the review.
`;

async function readManifest(file: File): Promise<string> {
  if (file.size > MAX_BYTES) throw new Error("Choose a team package smaller than 256 KB.");
  if (file.name.endsWith(".team.yaml")) return file.text();
  if (!file.name.endsWith(".team")) throw new Error("Choose a .team or .team.yaml file.");
  const bytes = await file.arrayBuffer();
  if (bytes.byteLength < 30) throw new Error("This archive is incomplete.");
  const header = new DataView(bytes);
  const filenameLength = header.getUint16(26, true);
  const start = 30 + filenameLength + header.getUint16(28, true);
  const compressedSize = header.getUint32(18, true);
  const size = header.getUint32(22, true);
  if (header.getUint32(0, true) !== 0x04034b50 || header.getUint16(6, true) & 9
      || start + compressedSize > bytes.byteLength || size > MAX_BYTES
      || new TextDecoder().decode(bytes.slice(30, 30 + filenameLength)) !== "team.yaml") {
    throw new Error("Choose a Domios .team export with team.yaml as its first file.");
  }
  const method = header.getUint16(8, true);
  if (method !== 0 || size !== compressedSize) throw new Error("Use the .team file exported by Domios or choose an inline .team.yaml.");
  return new TextDecoder("utf-8", { fatal: true }).decode(bytes.slice(start, start + size));
}

async function preview(content: string): Promise<SampleTeamPackage> {
  const { parseDocument } = await import("yaml");
  const document = parseDocument(content, { schema: "core", uniqueKeys: true });
  if (document.errors.length || document.warnings.length) throw new Error("The team manifest could not be read.");
  const value = document.toJS({ maxAliasCount: 0 });
  if (typeof value?.name !== "string" || !/^[a-z0-9.-]+\/[a-z0-9-]+$/.test(value.name)
    || typeof value.version !== "string" || typeof value.description !== "string" || value.roles?.orchestrator?.kind !== "manager") {
    throw new Error("This file needs an ATP name, version, description and Orchestrator role.");
  }
  const roles: SampleTeamRole[] = [];
  for (const [name, raw] of Object.entries(value.roles).slice(0, 12)) {
    const role = raw as { kind?: unknown; name?: unknown } | null;
    if (!/^[a-z][a-z0-9_]*$/.test(name) || (role?.kind !== "manager" && role?.kind !== "worker")) {
      throw new Error("The manifest contains an invalid role.");
    }
    roles.push({ name, kind: role.kind, title: typeof role.name === "string" ? role.name.slice(0, 50) : name.replace(/_/g, " ") });
  }
  const title = typeof value.title === "string" && value.title.trim() ? value.title.trim() : value.name.split("/").pop();
  return { title: title.slice(0, 70), roles };
}

export function TeamPackageInput({ onLoad }: { onLoad: (team: SampleTeamPackage) => void }) {
  const input = useRef<HTMLInputElement>(null);
  const [team, setTeam] = useState<SampleTeamPackage | null>(null);
  const [error, setError] = useState("");
  const latest = useRef(0);
  const load = async (content: Promise<string>) => {
    const request = ++latest.current;
    setError("");
    setTeam(null);
    try {
      const result = await preview(await content);
      if (request === latest.current) setTeam(result);
    } catch (reason) {
      if (request === latest.current) setError(reason instanceof Error ? reason.message : "Could not read this team package.");
    }
  };
  return <section className="relative mb-3 rounded-md border border-dashed border-[var(--ct-border)] p-3 text-xs" aria-label="Load new team"
    onDragOver={(event) => event.preventDefault()} onDrop={(event) => {
      event.preventDefault();
      const file = event.dataTransfer.files[0];
      if (file) void load(readManifest(file));
    }}>
    <input ref={input} type="file" accept=".team,.yaml" hidden onChange={(event) => {
      const file = event.currentTarget.files?.[0];
      if (file) void load(readManifest(file));
      event.currentTarget.value = "";
    }} />
    <div className="flex flex-wrap items-center gap-2">
      <Button size="sm" onClick={() => input.current?.click()}><FileUp size={13} /> Load new team</Button>
      <Button size="sm" onClick={() => void load(Promise.resolve(SAMPLE))}>Use sample .team</Button>
    </div>
    <p className="mt-1 text-[var(--ct-muted-fg)]">Drop a .team package to preview its team and roles.</p>
    {error && <p role="alert" className="mt-2 text-[var(--ct-red)]">{error}</p>}
    {team && <div className="mt-2 flex flex-wrap items-center justify-between gap-2">
      <span><strong>{team.title}</strong> · {team.roles.length} roles</span>
      <Button size="sm" variant="primary" onClick={() => onLoad(team)}>Start sample team</Button>
    </div>}
  </section>;
}
