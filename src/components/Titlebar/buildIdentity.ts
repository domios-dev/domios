export function developmentBuildLabel(isDevelopment: boolean, version: string): string | null {
  return isDevelopment ? `DEV · v${version}` : null;
}
