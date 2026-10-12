import { FrontendUpdateError } from "./bootstrapModel";
import type { CheckpointWrite } from "./checkpoint";
import { swapEvent, type SwapEvent, type SwapState, type Ticket } from "./model";

export interface FrontendHttp {
  json<T>(method: string, path: string, body?: unknown): Promise<T>;
}

export interface ActivationResponse {
  releaseId: string;
  generation: number;
  claims: Record<string, string>;
  index: string;
}

export function createFrontendUpdateClient(http: FrontendHttp, clientId: string) {
  if (!/^[a-zA-Z0-9_-]{1,128}$/.test(clientId)) throw new FrontendUpdateError("invalid-client");
  const checkpointPath = `/api/frontend/clients/${encodeURIComponent(clientId)}/checkpoint`;
  return {
    checkpoint: () => http.json<CheckpointWrite | null>("GET", checkpointPath),
    persist: (write: CheckpointWrite) => http.json<CheckpointWrite>("PUT", checkpointPath, write),
    async activate(releaseId: string): Promise<Ticket> {
      const response = await http.json<ActivationResponse>("POST", "/api/frontend/activate", { releaseId, clients: [clientId] });
      const token = response.claims[clientId];
      if (response.releaseId !== releaseId || !Number.isSafeInteger(response.generation) || !token) {
        throw new Error("Invalid frontend activation ticket");
      }
      return { releaseId, generation: response.generation, token };
    },
    healthy: (ticket: Ticket) => http.json("POST", "/api/frontend/healthy", { ...ticket, clientId }),
    rollback: (ticket: Ticket) => http.json("POST", "/api/frontend/rollback", { releaseId: ticket.releaseId, generation: ticket.generation }),
  };
}

export function releaseIndex(ticket: Ticket): string {
  if (!/^[a-zA-Z0-9_-][a-zA-Z0-9_.-]{0,127}$/.test(ticket.releaseId) || ticket.releaseId.endsWith(".") || !Number.isSafeInteger(ticket.generation) || ticket.generation < 1) {
    throw new Error("Invalid immutable frontend identity");
  }
  return `/frontend/${encodeURIComponent(ticket.releaseId)}/index.html?generation=${ticket.generation}`;
}

export function createSwapController(initial: SwapState, api: ReturnType<typeof createFrontendUpdateClient>, reloadPage: (ticket: Ticket) => void, onChange: (state: SwapState) => void = () => {}) {
  let state = initial;
  let disposed = false;
  const dispatch = (event: SwapEvent): void => {
    if (disposed) return;
    const transition = swapEvent(state, event);
    state = transition.state;
    onChange(state);
    for (const effect of transition.effects) {
      if (effect.type === "persist") {
        void api.persist(effect.write).then(write => dispatch({ type: "persisted", write }), () => dispatch({ type: "failed" }));
      } else if (effect.type === "activate") {
        void api.activate(effect.releaseId).then(ticket => dispatch({ type: "activated", ticket }), () => dispatch({ type: "failed" }));
      } else {
        reloadPage(effect.ticket);
      }
    }
  };
  return { dispatch, snapshot: () => state, dispose: () => { disposed = true; } };
}


export interface BootContext {
  apiOrigin: string;
  token: string;
  clientId: string;
  ticket: Ticket;
  attempted: string[];
  bundled: { index: string; minApiVersion: number; maxApiVersion: number };
}

export function preparePageReload(context: BootContext, storage: Pick<Storage, "setItem">, navigate: (path: string) => void): void {
  const path = releaseIndex(context.ticket);
  storage.setItem("codeterm.frontend.boot", JSON.stringify(context));
  navigate(path);
}
