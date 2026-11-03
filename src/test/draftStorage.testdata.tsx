import { useState, type ReactNode } from "react";
import { DraftPersistence } from "../lib/frontendUpdate/draftPersistence";
import { DraftPersistenceContext } from "../lib/usePersistentDraft";

export function DraftStorageFixture({ children }: { children: ReactNode }) {
  const [persistence] = useState(() => {
    const values = new Map<string, string>();
    return new DraftPersistence({
      read: async () => new Map(values),
      write: async (scope, value) => {
        if (value === null) values.delete(scope);
        else values.set(scope, value);
      },
    });
  });
  return <DraftPersistenceContext value={persistence}>{children}</DraftPersistenceContext>;
}
