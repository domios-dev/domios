export interface DesktopChallenge {
  code_challenge: string;
  state: string;
  redirect_uri: string;
}

export function validRedirect(value: unknown): value is string {
  if (value === "domios://auth/callback") return true;
  if (typeof value !== "string") return false;
  const match = /^http:\/\/127\.0\.0\.1:([1-9][0-9]{0,4})\/callback$/.exec(value);
  return match !== null && match[0] === value && Number(match[1]) <= 65535;
}

export function parseChallenge(value: unknown): DesktopChallenge {
  if (typeof value !== "object" || value === null) throw new Error("Invalid login request");
  const data = value as Record<string, unknown>;
  if (typeof data.code_challenge !== "string" || (data.code_challenge.length !== 43 || /[^A-Za-z0-9_-]/.test(data.code_challenge))
    || typeof data.state !== "string" || (data.state.length < 16 || data.state.length > 256 || /[^A-Za-z0-9_-]/.test(data.state))
    || !validRedirect(data.redirect_uri)) throw new Error("Invalid login request");
  return { code_challenge: data.code_challenge, state: data.state, redirect_uri: data.redirect_uri };
}

export function challengeFromQuery(query: URLSearchParams): DesktopChallenge {
  for (const key of ["code_challenge", "state", "redirect_uri"]) {
    if (query.getAll(key).length !== 1) throw new Error("Invalid login request");
  }
  return parseChallenge(Object.fromEntries(query));
}

export function callbackURL(challenge: DesktopChallenge, code: string): string {
  parseChallenge(challenge);
  if (code.length !== 43 || /[^A-Za-z0-9_-]/.test(code)) throw new Error("Invalid login request");
  const target = new URL(challenge.redirect_uri);
  target.search = new URLSearchParams({code, state: challenge.state}).toString();
  return target.toString();
}
