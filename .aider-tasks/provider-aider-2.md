# Fix: provider-aider tests must pass

Running `node scripts/build-plugin.mjs provider-aider && npx tsx plugins/provider-aider/tests/plugin.test.ts` fails:
TypeError: Cannot read properties of null (reading 'question') at plugins/provider-aider/src/plugin.ts:76 in the shell-command prompt classification (promptSafeChoice is called with a null prompt).
Fix plugins/provider-aider/src/plugin.ts so every exported function handles "no prompt" (null/undefined) by returning null, and make the whole test file pass. Do not weaken or delete test assertions; if a test is wrong about Aider's real screen output, fix the test to match Aider 0.86 output and say so in the commit message.
Commit message in English: "fix(provider-aider): handle no prompt and pass tests".
