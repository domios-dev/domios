export function isDevelopmentFrontendBuild(
  command: string,
  env: Readonly<Record<string, string | undefined>>,
): boolean {
  return command === "serve" || env.CODETERM_FRONTEND_DEV === "1";
}

export function developmentBuildLabel(isDevelopment: boolean, version: string): string | null {
  return isDevelopment ? `DEV · v${version}` : null;
}
