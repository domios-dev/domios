import { randomUUID } from "node:crypto";
import { readFileSync } from "node:fs";
import { initializeApp, deleteApp } from "firebase-admin/app";
import { getAuth } from "firebase-admin/auth";
import { getFirestore } from "firebase-admin/firestore";
import { initializeTestEnvironment, assertFails, type RulesTestEnvironment } from "@firebase/rules-unit-testing";
import { doc, getDoc, setDoc } from "firebase/firestore";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { FirebaseAuthProvider, FirebaseDataProvider } from "../src/adapters/firebase";
import { createAdminSession, grantAdminClaim } from "../src/domain/auth";
import { AdminService } from "../src/domain/service";
import type { Audit } from "../src/domain/model";

const projectId = "demo-domios-admin";
const app = initializeApp({ projectId }, "admin-emulator-test");
const auth = getAuth(app);
const db = getFirestore(app);
const backend = { auth: new FirebaseAuthProvider(auth), data: new FirebaseDataProvider(auth, db) };
const service = new AdminService(backend, ["pro"]);
let cookie: string;
let actor: string;
let uid: string;
let rules: RulesTestEnvironment;
beforeAll(async () => {
  if (!process.env.FIRESTORE_EMULATOR_HOST || !process.env.FIREBASE_AUTH_EMULATOR_HOST) throw new Error("Emulators required");
  actor = `actor-${randomUUID()}`; uid = `user-${randomUUID()}`;
  await auth.createUser({ uid: actor, email: `${actor}@example.com`, password: "emulator-password" });
  await auth.createUser({ uid, email: `${uid}@example.com` });
  await auth.setCustomUserClaims(actor, { beta: true });
  await grantAdminClaim(backend.auth, actor);
  const response = await fetch(`http://${process.env.FIREBASE_AUTH_EMULATOR_HOST}/identitytoolkit.googleapis.com/v1/accounts:signInWithPassword?key=fake`, {
    method: "POST", headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ email: `${actor}@example.com`, password: "emulator-password", returnSecureToken: true }),
  });
  expect(response.status).toBe(200);
  const { idToken } = await response.json();
  cookie = await createAdminSession(backend.auth, idToken);
  rules = await initializeTestEnvironment({ projectId, firestore: {
    host: "127.0.0.1", port: 28080, rules: readFileSync(new URL("../../firestore.rules", import.meta.url), "utf8"),
  } });
}, 30000);
afterAll(async () => { await rules?.cleanup(); await deleteApp(app); });
describe("persisted entitlement and audit transaction", () => {
  it("merges the owner bootstrap claim through the real adapter", async () => {
    expect((await auth.getUser(actor)).customClaims).toEqual({ beta: true, admin: true });
  });
  it("grant and revoke persist both records with before, after, actor and reason", async () => {
    const grant = await service.change(cookie, uid, "grant", "pro", "Support request");
    expect((await db.doc(`entitlements/${uid}`).get()).data()).toEqual(grant.after);
    expect((await db.doc(`adminAudit/${grant.id}`).get()).data()).toEqual(grant);
    expect(grant).toMatchObject({ before: null, actor, reason: "Support request", after: { plan: "pro", source: "admin", status: "active" } });
    const revoke = await service.change(cookie, uid, "revoke", "", "Refund issued");
    expect(revoke.before).toEqual(grant.after);
    expect(revoke).toMatchObject({ actor, reason: "Refund issued", after: { source: "admin", status: "revoked" } });
    expect((await db.doc(`entitlements/${uid}`).get()).data()).toEqual(revoke.after);
    expect((await db.doc(`adminAudit/${revoke.id}`).get()).data()).toEqual(revoke);
    expect((await db.doc(`adminAudit/${grant.id}`).get()).data()).toEqual(grant);
    expect((await service.detail(cookie, uid))?.entitlement).toEqual(revoke.after);
  });
  it("an existing audit id aborts the entitlement write", async () => {
    const existing = await service.change(cookie, uid, "grant", "pro", "Collision fixture");
    await expect(backend.data.change(uid, before => ({ ...existing, before, after: { ...existing.after, status: "revoked" } }))).rejects.toThrow();
    expect((await db.doc(`entitlements/${uid}`).get()).data()).toEqual(existing.after);
    expect((await db.doc(`adminAudit/${existing.id}`).get()).data()).toEqual(existing);
  });
  it("concurrent changes preserve a complete audit chain", async () => {
    const target = `concurrent-${randomUUID()}`;
    await auth.createUser({ uid: target });
    const entries = await Promise.all([
      service.change(cookie, target, "grant", "pro", "Concurrent grant"),
      service.change(cookie, target, "revoke", "", "Concurrent revoke"),
    ]);
    const first = entries.find(entry => entry.before === null)!;
    const second = entries.find(entry => entry.before !== null)!;
    expect(second.before).toEqual(first.after);
    expect((await db.doc(`entitlements/${target}`).get()).data()).toEqual(second.after);
    const persisted = await db.collection("adminAudit").where("uid", "==", target).get();
    expect(persisted.docs.map(record => record.data() as Audit)).toEqual(expect.arrayContaining(entries));
    expect(persisted.size).toBe(2);
  });
  it("browser clients including admins cannot access server collections", async () => {
    for (const context of [rules.unauthenticatedContext(), rules.authenticatedContext(uid), rules.authenticatedContext(actor, { admin: true })]) {
      for (const collection of ["entitlements", "adminAudit", "billingCustomers"]) {
        const ref = doc(context.firestore(), collection, uid);
        await assertFails(getDoc(ref)); await assertFails(setDoc(ref, { source: "admin" }));
      }
    }
  });
  it("removed admin claim denies reads and writes from an existing session", async () => {
    await auth.setCustomUserClaims(actor, { beta: true });
    await expect(service.detail(cookie, uid)).rejects.toMatchObject({ status: 403 });
    await expect(service.change(cookie, uid, "grant", "pro", "Unauthorized")).rejects.toMatchObject({ status: 403 });
  });
});
