export function quoteWindowsArgument(value: string): string {
  return '"' + value.replace(/(\\*)"/g, '$1$1\\"').replace(/\\+$/, '$&$&') + '"';
}

export function quoteNativeArgument(
  value: string,
  platform: string,
  quote: (value: string, platform: string) => string,
): string {
  const literal = quote(value, platform);
  if (platform !== "windows") return literal;
  // PowerShell before 7.3 passes a command line; Standard passes an argument list.
  return `$(if ($PSVersionTable.PSVersion -ge [version]'7.3') { ${literal} } else { ${quote(quoteWindowsArgument(value), platform)} })`;
}
