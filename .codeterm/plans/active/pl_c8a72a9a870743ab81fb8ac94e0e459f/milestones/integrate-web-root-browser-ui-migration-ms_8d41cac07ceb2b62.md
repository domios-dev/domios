---
plan_id: pl_c8a72a9a870743ab81fb8ac94e0e459f
kind: milestone
id: ms_8d41cac07ceb2b62
handle: integrate-web-root-browser-ui-migration-ms_8d41cac07ceb2b62
title: status
---

Integrated Polo web-root browser UI migration in the hot checkout. Root / serves the SPA, manifest/bootstrap/icons are root-scoped, CLI/Settings/Web UI links emit root URLs, Playwright helpers/specs/docs use root URLs, and /web plus /web/* now return 404 rather than acting as a legacy mount. Verified: pnpm exec tsx src/lib/webAccess.test.ts; cargo test --manifest-path cli/Cargo.toml web; cargo test --manifest-path src-tauri/Cargo.toml web_routes --lib; cargo fmt --check for cli/src-tauri; npx tsc --noEmit -p tsconfig.json; pnpm build; CODETERM_API_PORT=7686 ./build.sh check; PW_LINT_SCOPE=tests/playwright/suites/12-web pnpm test:pw:lint; live curl / 200, /manifest.webmanifest root-scope, /web and /web/anything 404; Playwright smoke @smoke / serves; Playwright web-access golden 4/4. Note: after dev restart, CODETERM_API_PORT=7686 plan list returned empty despite this repo plan file existing, so closure was recorded in the plan artifact directly as dogfood friction.
