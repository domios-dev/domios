export interface ResizeGeometry {
  cols: number;
  rows: number;
}

export interface ResizeAuthority {
  ptyId: string;
  clientId: string;
}

export interface ResizeRequest extends ResizeAuthority, ResizeGeometry {
  generation: number;
}

export interface ResizeModel {
  generation: number;
  owner: ResizeAuthority | null;
  applied: ResizeGeometry | null;
  inFlight: ResizeRequest | null;
  pending: { geometry: ResizeGeometry; force: boolean } | null;
}

export function createResizeModel(): ResizeModel {
  return { generation: 0, owner: null, applied: null, inFlight: null, pending: null };
}

function sameGeometry(left: ResizeGeometry | null, right: ResizeGeometry): boolean {
  return left?.cols === right.cols && left.rows === right.rows;
}

export function setResizeAuthority(
  state: ResizeModel,
  owner: ResizeAuthority | null,
  applied: ResizeGeometry | null = null,
): ResizeModel {
  return {
    ...state,
    generation: state.generation + 1,
    owner,
    applied: owner && !state.inFlight ? applied : null,
    pending: null,
  };
}

function drain(state: ResizeModel): ResizeModel {
  if (state.inFlight || !state.owner || !state.pending) return state;
  const { geometry, force } = state.pending;
  if (!force && sameGeometry(state.applied, geometry)) return { ...state, pending: null };
  return {
    ...state,
    pending: null,
    inFlight: { ...state.owner, ...geometry, generation: state.generation },
  };
}

export function requestResize(state: ResizeModel, geometry: ResizeGeometry, force = false): ResizeModel {
  if (!state.owner || geometry.cols <= 0 || geometry.rows <= 0) return state;
  return drain({ ...state, pending: { geometry, force } });
}

export function completeResize(state: ResizeModel, request: ResizeRequest, succeeded: boolean): ResizeModel {
  if (state.inFlight !== request) return state;
  const current = request.generation === state.generation;
  return drain({
    ...state,
    inFlight: null,
    applied: current ? (succeeded ? { cols: request.cols, rows: request.rows } : null) : state.applied,
    pending: current && !succeeded ? null : state.pending,
  });
}
