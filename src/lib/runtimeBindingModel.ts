export function createRuntimeBinding<Client>(bind: (client: Client) => void, invalidate: () => void): (client: Client) => void {
  let current: Client;
  let initialized = false;
  return (client) => {
    if (initialized && current === client) return;
    if (initialized) invalidate();
    bind(client);
    current = client;
    initialized = true;
  };
}
