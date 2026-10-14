import { getFirestore, Timestamp } from "firebase-admin/firestore";
import { getAuth } from "firebase-admin/auth";
import type { CodeStore, LoginCode, TokenMinter } from "../../loginCodes";

export class FirestoreCodeStore implements CodeStore {
  async insert(hash: string, record: LoginCode): Promise<void> {
    await getFirestore().doc(`desktopLoginCodes/${hash}`).create({...record,expires_at:Timestamp.fromMillis(record.expires_at)});
  }

  async consume(hash: string, validate: (record: LoginCode | undefined) => string): Promise<string> {
    const db = getFirestore();
    const ref = db.doc(`desktopLoginCodes/${hash}`);
    return db.runTransaction(async (transaction) => {
      const data = (await transaction.get(ref)).data();
      const record = data ? {...data, expires_at:data.expires_at instanceof Timestamp ? data.expires_at.toMillis() : NaN} as LoginCode : undefined;
      const uid = validate(record);
      transaction.update(ref, {used:true});
      return uid;
    });
  }

  async allowAttempt(ipHash: string, now: number, limit: number, windowMs: number): Promise<boolean> {
    const db = getFirestore();
    const ref = db.doc(`desktopLoginRateLimits/${ipHash}`);
    return db.runTransaction(async (transaction) => {
      const current = (await transaction.get(ref)).data();
      const active = current && current.expires_at instanceof Timestamp && current.expires_at.toMillis() > now;
      const count = active ? current.count : 0;
      if (count >= limit) return false;
      transaction.set(ref, {count:count+1,expires_at:active ? current.expires_at : Timestamp.fromMillis(now+windowMs)});
      return true;
    });
  }
}

export class AdminTokenMinter implements TokenMinter {
  mint(uid: string): Promise<string> { return getAuth().createCustomToken(uid); }
}
