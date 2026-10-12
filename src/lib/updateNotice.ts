export interface UpdateNoticeInput {
  title: string;
  body: string;
  failed: boolean;
  error: string | null;
  latestVersion: string | null;
}

export function updateNotice(input: UpdateNoticeInput) {
  return {
    title: input.latestVersion ? `${input.title} v${input.latestVersion}` : input.title,
    body: input.error || input.body,
    severity: input.failed ? "Error" as const : "Info" as const,
    level: input.failed ? "failure" as const : "verbose" as const,
    retry: input.failed,
  };
}
