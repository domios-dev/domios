// Caller: scripts/dev-prep.mjs; scripts/tests/dev-runtime-environment.test.mjs
import { join } from 'node:path';

export function devRuntimeEnvironment(inherited, physicalRepo) {
  return {
    ...inherited,
    CODETERM_DEV_WORKSPACE_ROOT: physicalRepo,
    CODETERM_WEB_DIST: join(physicalRepo, 'dist'),
  };
}
