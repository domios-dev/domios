export function createPageLoadCache<Key, Value>(load: (key: Key) => Promise<Value>) {
  const entries = new Map<string, Promise<Value>>();
  return {
    read(key: Key): Promise<Value> {
      const id = JSON.stringify(key);
      let pending = entries.get(id);
      if (!pending) {
        pending = load(key);
        entries.set(id, pending);
        void pending.catch(() => { if (entries.get(id) === pending) entries.delete(id); });
      }
      return pending;
    },
    async refresh(matches: (key: Key) => boolean) {
      const keys = [...entries.keys()].map((id) => JSON.parse(id) as Key).filter(matches);
      for (const key of keys) entries.delete(JSON.stringify(key));
      await Promise.all(keys.map((key) => this.read(key)));
    },
    clear() { entries.clear(); },
  };
}
