import { useEffect, useState } from "react";
import { invoke } from "../../lib/bridge";
import type { AutomationIdentity } from "./types";
import type { NodeFile } from "./nodeFilesModel";

export interface NodeFilesAdapter {
  read(identity: AutomationIdentity, bucket: string | null, handler: string | null): Promise<NodeFile[]>;
  open(path: string): Promise<unknown>;
  reveal(path: string): Promise<unknown>;
}

export const nodeFilesAdapter: NodeFilesAdapter = {
  read: (identity, bucket, handler) => invoke("automation_node_files", { identity, bucket, handler }),
  open: (path) => invoke("open_folder", { path }),
  reveal: (path) => invoke("reveal_file", { path }),
};

export function useNodeFiles(adapter: NodeFilesAdapter, identity: AutomationIdentity, bucket: string | null, handler: string | null) {
  const key = JSON.stringify([identity, bucket, handler]);
  const [result, setResult] = useState<{ key: string; files: NodeFile[]; failed: boolean } | null>(null);
  useEffect(() => {
    let current = true;
    const [target, route, action]: [AutomationIdentity, string | null, string | null] = JSON.parse(key);
    void adapter.read(target, route, action).then(
      (files) => { if (current) setResult({ key, files, failed: false }); },
      () => { if (current) setResult({ key, files: [], failed: true }); },
    );
    return () => { current = false; };
  }, [adapter, key]);
  return result?.key === key ? result : null;
}
