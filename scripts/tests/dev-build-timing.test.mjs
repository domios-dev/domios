import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';

const source = readFileSync(new URL('../../build.sh', import.meta.url), 'utf8');
const helper = source.match(/^dev_build_phase\(\) \{[^]*?^\}/m)[0];
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';

for (const command of ['dev', 'dev-back', 'dev-prebuilt', 'dev-server', 'release-bundle']) {
  for (const enabled of ['0', '1']) {
    test(`DEV timing emits fixed phase boundaries only: ${command}, enabled=${enabled}`, () => {
      const result = spawnSync(bash, [], { encoding: 'utf8', input: `set -euo pipefail
${helper}
CMD=${command} CODETERM_BUILD_TIMINGS=${enabled}
export CODETERM_API_TOKEN=opaque-credential DOMIOS_FIREBASE_API_KEY=opaque-server-key
dev_build_phase cargo-cli.begin opaque-argument
printf opaque-command-result
dev_build_phase cargo-cli.end opaque-argument
` });
      assert.equal(result.status, 0, result.stderr);
      assert.equal(result.stdout, 'opaque-command-result');
      if (enabled === '0' || command === 'release-bundle') {
        assert.equal(result.stderr, '');
      } else {
        const rows = result.stderr.trim().split(/\r?\n/);
        assert.equal(rows.length, 2);
        for (const [index, boundary] of ['begin', 'end'].entries()) {
          assert.match(rows[index], new RegExp(`^\\[dev-phase\\] time=[0-9]+\\.[0-9]+ pid=[0-9]+ phase=cargo-cli\\.${boundary}$`));
        }
        assert.doesNotMatch(result.stderr, /opaque|credential|TOKEN|KEY/);
      }
    });
  }
}
