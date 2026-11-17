import fs from 'node:fs';
import path from 'node:path';

if (process.env.GITHUB_ACTIONS !== 'true') throw new Error('Stable checkout relocation is restricted to GitHub Actions');
if (process.platform === 'win32') throw new Error('Windows uses the gateway X: mapping');

const workspace = path.resolve(process.env.GITHUB_WORKSPACE);
const base = process.platform === 'darwin' ? '/private/tmp/domios-ci' : '/tmp/domios-ci';
const checkout = path.join(base, 'checkout');
const receipt = path.join(base, 'owner.json');
const identity = {
  workspace,
  run: process.env.GITHUB_RUN_ID,
  attempt: process.env.GITHUB_RUN_ATTEMPT,
  job: process.env.GITHUB_JOB,
};

switch (process.argv[2]) {
  case 'enter': {
    if (!fs.lstatSync(workspace).isDirectory() || !fs.lstatSync(path.join(workspace, '.git')).isDirectory()) {
      throw new Error('Stable CI checkout requires a physical standalone clone');
    }
    fs.mkdirSync(base, { mode: 0o700 });
    fs.writeFileSync(receipt, JSON.stringify(identity), { flag: 'wx', mode: 0o600 });
    let moved = false;
    try {
      fs.renameSync(workspace, checkout);
      moved = true;
      fs.symlinkSync(checkout, workspace, 'dir');
      const target = process.env.CARGO_TARGET_DIR;
      const relative = target && path.relative(workspace, target);
      if (target && (relative.startsWith('..') || path.isAbsolute(relative))) {
        throw new Error('CI target must belong to the job checkout');
      }
      const stableTarget = target ? path.join(checkout, relative) : path.join(checkout, 'src-tauri/target');
      fs.appendFileSync(process.env.GITHUB_ENV,
        `CODETERM_CI_STABLE_CHECKOUT=1\nCARGO_TARGET_DIR=${stableTarget}\nCARGO_BUILD_BUILD_DIR=${path.join(stableTarget, '.build')}\n`);
    } catch (error) {
      if (fs.existsSync(workspace) && fs.lstatSync(workspace).isSymbolicLink()) fs.unlinkSync(workspace);
      if (moved) fs.renameSync(checkout, workspace);
      fs.unlinkSync(receipt);
      fs.rmdirSync(base);
      throw error;
    }
    console.log(`CI physical checkout: ${checkout}`);
    break;
  }
  case 'leave': {
    if (process.env.CODETERM_CI_STABLE_CHECKOUT !== '1') break;
    const owner = JSON.parse(fs.readFileSync(receipt, 'utf8'));
    if (JSON.stringify(owner) !== JSON.stringify(identity) || !fs.lstatSync(workspace).isSymbolicLink() ||
        fs.readlinkSync(workspace) !== checkout) throw new Error('Stable checkout owner changed; refusing cleanup');
    process.chdir(path.dirname(workspace));
    fs.unlinkSync(workspace);
    fs.renameSync(checkout, workspace);
    fs.unlinkSync(receipt);
    fs.rmdirSync(base);
    console.log('CI checkout restored to runner workspace');
    break;
  }
  default: throw new Error('Usage: node scripts/ci-stable-checkout.mjs enter|leave');
}
