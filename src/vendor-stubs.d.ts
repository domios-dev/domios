// Minimal stubs so `tsc --noEmit` passes before `npm install sigma graphology`.
// Once installed, the real bundled types from each package shadow these.
// Safe to delete after deps are installed.

declare module "sigma" {
  export interface SigmaSettings {
    renderLabels?: boolean;
    renderEdgeLabels?: boolean;
    defaultNodeType?: string;
    defaultEdgeType?: string;
  }
  export interface SigmaCamera {
    setState(s: { ratio?: number; x?: number; y?: number }): void;
  }
  export default class Sigma {
    constructor(graph: unknown, container: HTMLElement, settings?: SigmaSettings);
    kill(): void;
    refresh(): void;
    getCamera(): SigmaCamera;
    on(event: string, cb: (payload: { node: string }) => void): void;
  }
}

declare module "graphology" {
  export default class Graph {
    constructor();
    clear(): void;
    addNode(id: string, attrs: Record<string, unknown>): void;
    addEdge(from: string, to: string, attrs: Record<string, unknown>): void;
    hasNode(id: string): boolean;
  }
}
