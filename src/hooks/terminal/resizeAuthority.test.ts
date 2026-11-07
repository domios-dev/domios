import { describe, expect, it } from "vitest";
import {
  authorityAfterClaim,
  authorityAfterOwnerChanged,
  claimModeFor,
  renderGeometry,
  unclaimedAuthority,
  type ViewerAuthority,
} from "./resizeAuthority";

const self = { clientId: "client:web", ptyId: "pty-1" };
const fitted = { cols: 163, rows: 46 };

describe("resize authority claim mode", () => {
  it("takes the PTY only on explicit interaction with this view", () => {
    expect(claimModeFor("interaction")).toBe("forced");
  });

  it("adopts an unowned PTY on activation but never steals an owned one", () => {
    expect(claimModeFor("activation")).toBe("unforced");
    expect(claimModeFor("visibility")).toBe("unforced");
    expect(claimModeFor("window-focus")).toBe("unforced");
  });

  it("never claims from a passive render, a background view or a reconnect", () => {
    expect(claimModeFor("passive-render")).toBe("none");
    expect(claimModeFor("background")).toBe("none");
    expect(claimModeFor("reconnect")).toBe("none");
  });
});

describe("resize authority after a claim answer", () => {
  it("owns the PTY when the daemon names this client", () => {
    const authority = authorityAfterClaim(
      { ownerClientId: self.clientId, ptyId: self.ptyId, ownerSize: { cols: 120, rows: 40 }, currentSize: { cols: 120, rows: 40 } },
      self,
    );
    expect(authority).toEqual<ViewerAuthority>({ role: "owner", followed: null });
    expect(renderGeometry(authority, fitted)).toEqual(fitted);
  });

  it("follows the PTY's current size when another client owns it", () => {
    const authority = authorityAfterClaim(
      { ownerClientId: "client:native", ptyId: self.ptyId, ownerSize: { cols: 138, rows: 46 }, currentSize: { cols: 140, rows: 44 } },
      self,
    );
    expect(authority).toEqual<ViewerAuthority>({ role: "follower", followed: { cols: 140, rows: 44 } });
    expect(renderGeometry(authority, fitted)).toEqual({ cols: 140, rows: 44 });
  });

  it("falls back to the owner's claimed size when the PTY size is unknown", () => {
    const authority = authorityAfterClaim(
      { ownerClientId: "client:native", ptyId: self.ptyId, ownerSize: { cols: 138, rows: 46 }, currentSize: null },
      self,
    );
    expect(authority.followed).toEqual({ cols: 138, rows: 46 });
  });

  it("stays unclaimed when the answer is about another PTY", () => {
    const authority = authorityAfterClaim(
      { ownerClientId: self.clientId, ptyId: "pty-other", ownerSize: { cols: 80, rows: 24 }, currentSize: null },
      self,
    );
    expect(authority).toEqual(unclaimedAuthority());
    expect(renderGeometry(authority, fitted)).toEqual(fitted);
  });
});

describe("resize authority after an owner-changed event", () => {
  const owner: ViewerAuthority = { role: "owner", followed: null };

  it("demotes the owner to a follower of the new owner's size", () => {
    const next = authorityAfterOwnerChanged(owner, { client_id: "client:native", pty_id: self.ptyId, cols: 88, rows: 34 }, self);
    expect(next).toEqual<ViewerAuthority>({ role: "follower", followed: { cols: 88, rows: 34 } });
  });

  it("tracks the owner's later resizes while following", () => {
    const following: ViewerAuthority = { role: "follower", followed: { cols: 88, rows: 34 } };
    const next = authorityAfterOwnerChanged(following, { client_id: "client:native", pty_id: self.ptyId, cols: 138, rows: 46 }, self);
    expect(next.followed).toEqual({ cols: 138, rows: 46 });
    expect(renderGeometry(next, fitted)).toEqual({ cols: 138, rows: 46 });
  });

  it("ignores events about this client or another PTY", () => {
    expect(authorityAfterOwnerChanged(owner, { client_id: self.clientId, pty_id: self.ptyId, cols: 1, rows: 1 }, self)).toBe(owner);
    expect(authorityAfterOwnerChanged(owner, { client_id: "client:native", pty_id: "pty-other", cols: 1, rows: 1 }, self)).toBe(owner);
  });

  it("ignores invalid sizes", () => {
    const following: ViewerAuthority = { role: "follower", followed: { cols: 88, rows: 34 } };
    expect(authorityAfterOwnerChanged(following, { client_id: "client:native", pty_id: self.ptyId, cols: 0, rows: 34 }, self)).toBe(following);
    expect(renderGeometry(following, { cols: 0, rows: 0 })).toEqual({ cols: 88, rows: 34 });
  });
});
