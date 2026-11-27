import { describe, expect, it } from "vitest";
import { FIRST_RUN_GA_TAB_MARKER } from "../../components/AgentPicker/constants";
import { RELAY_PROTOCOL_VERSION, type MultiplexerState, type PairedPeer, type Tab } from "../../lib/types";
import {
  observedGeneralViewers,
  pairedAvailability,
  pairedPeerIdentity,
  projectPairedGeneral,
} from "./pairedWorkspaceProjection";

const PEER_ID = "3efa6b47-cbad-4243-b381-86e57930319a";
const INST_ID = "e45c9e4ff42143f0bee45f1f75b11a9c";

function makePeer(overrides: Partial<PairedPeer> = {}): PairedPeer {
  return {
    instance_id: PEER_ID,
    instance_name: PEER_ID,
    addresses: [],
    api_port: 0,
    public_key: "pk-cloud",
    paired_at: "2026-01-01T00:00:00Z",
    last_seen: null,
    online: true,
    is_primary: false,
    access_level: "full",
    web_ui: null,
    last_active_ms: null,
    relay_protocol_version: RELAY_PROTOCOL_VERSION,
    ...overrides,
  };
}

describe("pairedWorkspaceProjection", () => {
  it("derives pairedAvailability from peer liveness, protocol version, and sign-out state", () => {
    const installations = [
      {
        installation_id: INST_ID,
        instance_id: PEER_ID,
        name: "",
        active: true,
        signed_out: false,
        online: true,
        membership_expires_at: null,
        endpoint_expires_at: null,
      },
    ];
    // Online inbound-only cloud peer starts in "reconnecting" so usePairedWorkspaces fetches its inventory.
    expect(pairedAvailability(makePeer({ online: true }), installations)).toBe("reconnecting");
    // Detached stream (online: false) degrades to "offline".
    expect(pairedAvailability(makePeer({ online: false }), installations)).toBe("offline");
    // Protocol mismatch requires update.
    expect(pairedAvailability(makePeer({ online: true, relay_protocol_version: 1 }), installations)).toBe("update-required");
    // Signed-out installation wins over online flag.
    expect(
      pairedAvailability(makePeer({ online: true }), [{ ...installations[0], signed_out: true }]),
    ).toBe("signed-out");
  });

  it("uses the cloud machine name in pairedPeerIdentity instead of falling back to Machine", () => {
    const identity = pairedPeerIdentity(
      {
        instance_id: PEER_ID,
        instance_name: PEER_ID,
        kind: "cloud",
        cloud: { name: "quill-fly-124-go" },
      },
      [{
        installation_id: INST_ID,
        instance_id: PEER_ID,
        name: "",
        active: true,
        signed_out: false,
        online: true,
        membership_expires_at: null,
        endpoint_expires_at: null,
      }],
    );
    expect(identity.peerId).toBe(PEER_ID);
    expect(identity.name).toBe("quill-fly-124-go");
    expect(identity.installation?.installation_id).toBe(INST_ID);
  });

  it("keeps a protected first-run Main tab visible in projectPairedGeneral and observedGeneralViewers", () => {
    const firstRunMain: Tab = {
      id: "tab-first-run-main",
      pty_id: null,
      tab_type: "Choice",
      title: "Main",
      description: FIRST_RUN_GA_TAB_MARKER,
      cwd: "/home/user/.domios/general",
      agent_type: null,
      agent_session_id: null,
      workspace_id: "ws-general",
      protected: true,
    };
    const unprotectedPicker: Tab = {
      ...firstRunMain,
      id: "tab-unprotected-picker",
      title: "Picker",
      protected: false,
    };
    const snapshot: MultiplexerState = {
      workspaces: [
        {
          id: "ws-general",
          name: "General",
          cwd: "/home/user/.domios/general",
          tabs: [firstRunMain.id, unprotectedPicker.id],
          active_tab_id: firstRunMain.id,
          protected: true,
        },
      ],
      active_workspace_id: "ws-general",
      tabs: {
        [firstRunMain.id]: firstRunMain,
        [unprotectedPicker.id]: unprotectedPicker,
      },
      focused_tab_id: firstRunMain.id,
    };

    const sessions = projectPairedGeneral(PEER_ID, snapshot);
    expect(sessions.map((s) => s.tab.id)).toEqual([firstRunMain.id]);
    expect(sessions[0].viewerId).toBe(`mesh:${PEER_ID}/${firstRunMain.id}`);

    const attachedViewer: Tab = {
      ...firstRunMain,
      id: `mesh:${PEER_ID}/${firstRunMain.id}`,
      workspace_id: "local-general",
      remote_peer_id: PEER_ID,
      remote_tab_id: firstRunMain.id,
    };
    const viewers = observedGeneralViewers(
      [{ peerId: PEER_ID, name: "Cloud Box", availability: "online", sessions: [] }],
      { [attachedViewer.id]: attachedViewer },
      "local-general",
    );
    expect(viewers).toHaveLength(1);
    expect(viewers[0].session.viewerId).toBe(attachedViewer.id);
  });
});
