// Caller: scripts/dev-prep.mjs; scripts/tests/dev-runtime-environment.test.mjs
import { join } from 'node:path';

export function devRuntimeEnvironment(inherited, physicalRepo) {
  const runtime = Object.fromEntries(Object.entries(inherited).filter(([key]) => !key.startsWith('CODETERM_DEV_LAUNCH_')));
  return {
    ...runtime,
    CODETERM_DEV_WORKSPACE_ROOT: physicalRepo,
    CODETERM_WEB_DIST: join(physicalRepo, 'dist'),
  };
}
