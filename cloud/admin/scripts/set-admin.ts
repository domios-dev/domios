import { firebaseBackend } from "../src/adapters/firebase";
import { grantAdminClaim } from "../src/domain/auth";
import { validUid } from "../src/domain/model";

const uid = process.argv[2];
if (process.argv.length !== 3 || !uid || !validUid(uid)) {
  process.stderr.write("Usage: set-admin.ts <uid>\n");
  process.exitCode = 1;
} else {
  try {
    await grantAdminClaim(firebaseBackend().auth, uid);
    process.stdout.write(`Admin claim granted to ${uid}; other claims preserved.\n`);
  } catch {
    process.stderr.write("Unable to grant admin claim. Check uid, project and owner credentials.\n");
    process.exitCode = 1;
  }
}
