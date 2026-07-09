import { strict as assert } from "node:assert";
import { terminalOwnsLivePty } from "./terminalOwnership";

assert.equal(
  terminalOwnsLivePty({
    isWeb: false,
    terminalVisible: true,
    paneVisible: false,
    ptyId: null,
  }),
  false,
  "hidden restored desktop pane without pty must not spawn a shell",
);

assert.equal(
  terminalOwnsLivePty({
    isWeb: false,
    terminalVisible: true,
    paneVisible: true,
    ptyId: null,
  }),
  true,
  "visible desktop pane may spawn its first shell",
);

assert.equal(
  terminalOwnsLivePty({
    isWeb: false,
    terminalVisible: true,
    paneVisible: false,
    ptyId: "pty-1",
  }),
  true,
  "hidden pane with an existing pty may keep its reader alive",
);

assert.equal(
  terminalOwnsLivePty({
    isWeb: true,
    terminalVisible: false,
    paneVisible: true,
    ptyId: null,
  }),
  false,
  "web chat mode must not own terminal IO",
);

assert.equal(
  terminalOwnsLivePty({
    isWeb: true,
    terminalVisible: true,
    paneVisible: true,
    ptyId: null,
  }),
  true,
  "web terminal mode may own terminal IO when visible",
);
