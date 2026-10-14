export interface AnalyticsStatus {
  consent: "off" | "on";
  queued: number;
  has_device_id: boolean;
  has_user_id: boolean;
}

export interface PrivacyApi {
  status(): Promise<AnalyticsStatus>;
  consent(value: "off" | "on"): Promise<AnalyticsStatus>;
}

export function analyticsEnabled(status: AnalyticsStatus | null): boolean {
  return status?.consent === "on";
}

export async function changeAnalyticsConsent(api: PrivacyApi, enabled: boolean): Promise<AnalyticsStatus> {
  return api.consent(enabled ? "on" : "off");
}
