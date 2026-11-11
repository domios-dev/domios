import type { KeepAwakeStatus } from "../../../gen/KeepAwakeStatus";

export interface KeepAwakePorts {
  status(): Promise<KeepAwakeStatus>;
  start(durationSecs: number | undefined, display: boolean): Promise<KeepAwakeStatus>;
  stop(): Promise<void>;
  saveDisplay(display: boolean): Promise<void>;
  now(): number;
}

export type KeepAwakeAction =
  | { kind: "start"; seconds: number | null; display: boolean }
  | { kind: "stop" }
  | { kind: "display"; display: boolean };

export async function changeKeepAwake(action: KeepAwakeAction, ports: KeepAwakePorts) {
  if (action.kind === "start") {
    return { status: await ports.start(action.seconds ?? undefined, action.display) };
  }
  if (action.kind === "stop") {
    await ports.stop();
    return { status: await ports.status() };
  }
  let status = await ports.status();
  if (status.active) {
    const seconds = status.expires_at === null ? undefined
      : Math.max(1, Math.ceil((status.expires_at - ports.now()) / 1000));
    status = await ports.start(seconds, action.display);
  }
  await ports.saveDisplay(action.display);
  return { status, displayPreference: action.display };
}
