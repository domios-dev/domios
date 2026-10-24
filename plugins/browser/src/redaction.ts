const CREDENTIALS = new Set(["token", "access_token", "api_token", "key", "secret", "password"]);

export function redactCredentials(text: string): string {
  return text.replace(/(^|[?#&;\s"'<>`()\[\]{}\\])([^=?#&;\s"'<>`()\[\]{}\\]+)=([^?#&;\s"'<>`()\[\]{}\\]*)/g,
    (match, prefix: string, name: string) => {
      let decoded: string;
      try { decoded = decodeURIComponent(name.replace(/\+/g, " ")).toLowerCase(); }
      catch { return match; }
      return CREDENTIALS.has(decoded) ? `${prefix}${name}=REDACTED` : match;
    });
}
