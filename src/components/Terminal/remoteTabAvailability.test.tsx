import { describe, expect, finish, it } from "../../test/standaloneTest";
import { renderToStaticMarkup } from "react-dom/server";
import type { Tab } from "../../lib/types";
import { RemoteTabUnavailable } from "./RemoteTabUnavailable";

const tab: Tab = {
  id: "mesh:peer/one", title: "One", pty_id: "one", tab_type: "Terminal", cwd: null,
  remote_peer_id: "peer", remote_peer_name: "ROG", remote_tab_id: "one",
  remote_availability: "reconnecting",
};

describe("remote terminal availability", () => {
  it("renders honest reconnect state with peer identity", () => {
    const html = renderToStaticMarkup(<RemoteTabUnavailable tab={tab} />);
    expect(html).toContain("Reconnecting · @ROG");
    expect(html).toContain('role="status"');
    const unavailable = renderToStaticMarkup(
      <RemoteTabUnavailable tab={{ ...tab, remote_availability: "unavailable" }} />,
    );
    expect(unavailable).toContain("Unavailable · @ROG");
  });
});

finish();
