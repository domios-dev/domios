import { strict as assert } from "node:assert";
import { terminalOwnsLivePty, terminalRenderEnabled } from "./terminalOwnership";

assert.equal(
  terminalOwnsLivePty({
    isWeb: false,
    terminalVisible: true,
    tabVisible: false,
    ptyId: null,
  }),
  false,
  "hidden restored desktop Tab without pty must not spawn a shell",
);

assert.equal(
  terminalOwnsLivePty({
    isWeb: false,
    terminalVisible: true,
    tabVisible: true,
    ptyId: null,
  }),
  true,
  "visible desktop Tab may spawn its first shell",
);

assert.equal(
  terminalOwnsLivePty({
    isWeb: false,
    terminalVisible: true,
    tabVisible: false,
    ptyId: "pty-1",
  }),
  true,
  "hidden Tab with an existing pty may keep its reader alive",
);

assert.equal(
  terminalRenderEnabled({ terminalVisible: true, tabVisible: false }),
  false,
  "hidden Tab must not render terminal output",
);

assert.equal(
  terminalOwnsLivePty({
    isWeb: true,
    terminalVisible: false,
    tabVisible: true,
    ptyId: null,
  }),
  false,
  "web chat mode must not own terminal IO",
);

assert.equal(
  terminalOwnsLivePty({
    isWeb: true,
    terminalVisible: true,
    tabVisible: true,
    ptyId: null,
  }),
  true,
  "web terminal mode may own terminal IO when visible",
);
