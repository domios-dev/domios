import { useEffect, useRef, useState } from "react";
import { signalFrontendUploads } from "../../lib/frontendUpdate/appState";
import { createComposerUploads, type ComposerUpload } from "./composerUploadModel";
import { storeBlob } from "./voice/audio";

export function useComposerUploads(deps: Omit<Parameters<typeof createComposerUploads>[0], "store" | "publish">) {
  const depsRef = useRef(deps);
  depsRef.current = deps;
  const [uploads, setUploads] = useState<ComposerUpload[]>([]);
  const owner = useRef({}).current;
  const controllerRef = useRef<ReturnType<typeof createComposerUploads> | null>(null);
  const controller = () => controllerRef.current ??= createComposerUploads({
    registry: () => depsRef.current.registry(), commit: (registry) => depsRef.current.commit(registry),
    insert: (capsule) => depsRef.current.insert(capsule),
    store: storeBlob,
    publish: (next) => {
      signalFrontendUploads(owner, next.filter((item) => item.state === "uploading").length);
      setUploads(next);
    },
  });
  useEffect(() => () => {
    controllerRef.current?.dispose();
    controllerRef.current = null;
    signalFrontendUploads(owner, 0);
  }, [owner]);
  return {
    uploads,
    add: (blob: Blob, name: string) => controller().add(blob, name),
    addFiles: async (files: Iterable<File>) => { await Promise.all(Array.from(files, (file) => controller().add(file, file.name || "file.bin"))); },
    retry: (id: number) => controller().retry(id),
    remove: (id: number) => controller().remove(id),
    clear: () => { for (const upload of uploads) controller().remove(upload.id); },
  };
}
