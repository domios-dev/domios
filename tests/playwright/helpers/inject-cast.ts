import type { ApiClient } from "./api-client";

export const CAST = {
  claudeSystemsDesignLongChatIdle:
    "claude/systems_design_long_chat_idle_v1-canonical-20260621-systems-chat-r2/raw.cast",
  syntheticPermissionBoxVisibleCursor: "synthetic/permission_box_visible_cursor_v1/raw.cast",
} as const;

export type CastPath = (typeof CAST)[keyof typeof CAST] | string;

export interface InjectCastOptions {
  tabId: string;
  castPath?: CastPath;
  castText?: string;
  speed?: number;
  instant?: boolean;
  classify?: boolean;
  applyResize?: boolean;
}

export interface InjectCastResult {
  ok: true;
  mode: "instant" | "paced";
  pty_id: string;
  bytes?: number;
  started?: boolean;
  start_offset: number;
  end_offset: number;
  planned_bytes: number;
  speed?: number;
  classify?: boolean;
}

export async function injectCast(
  api: Pick<ApiClient, "post">,
  options: InjectCastOptions,
): Promise<{ startOffset: number; endOffset: number; response: InjectCastResult }> {
  const body: Record<string, unknown> = {
    tab_id: options.tabId,
    speed: options.speed,
    instant: options.instant,
    classify: options.classify,
    apply_resize: options.applyResize,
  };

  if (options.castPath) {
    body.cast_path = options.castPath;
  }
  if (options.castText) {
    body.cast_b64 = Buffer.from(options.castText, "utf8").toString("base64");
  }

  const response = await api.post<InjectCastResult>("/dev/inject-cast", body);
  return {
    startOffset: response.start_offset,
    endOffset: response.end_offset,
    response,
  };
}
