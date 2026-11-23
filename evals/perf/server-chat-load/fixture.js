const sessions = new Map();
const chunkSize = 24;
const toolOutput = "fixture output\n".repeat(96);

function row(session, type, content, fields) {
  const seq = session.next++;
  return { id: `fixture-${seq}`, type, content, seq, ...fields };
}

function tool(session, content) {
  return row(session, "tool_call", content, {
    toolName: "FixtureCommand",
    toolKind: "command",
    toolInput: { command: "fixture" },
    toolStatus: "completed",
    toolResult: toolOutput,
  });
}

module.exports = {
  openSession({ tabId, config }) {
    const { seedRows, batches } = config;
    if (!Number.isSafeInteger(seedRows) || seedRows < 0 || seedRows > 2000 ||
        !Number.isSafeInteger(batches) || batches < 2 || batches > 1500) {
      throw new Error("Invalid fixture dimensions");
    }
    sessions.set(tabId, { seedRows, batches, next: 0, frames: [], turn: null });
    return { sessionId: tabId };
  },

  sendMessage(id, text) {
    const session = sessions.get(id);
    if (session.turn || session.seedRows !== 0 || !/^fixture-turn-[a-f0-9-]+$/.test(text)) {
      throw new Error("Unexpected fixture turn");
    }
    session.turn = { text, index: 0 };
    return null;
  },

  pump() { return null; },

  poll(id, cursor) {
    const session = sessions.get(id);
    // Reads without a cursor hydrate history without consuming the next live batch.
    if (cursor === null && session.frames.length) {
      return { messages: session.frames.flat(), cursor: String(session.frames.length), done: !session.turn };
    }
    if (Number(cursor) < session.frames.length) {
      return {
        messages: session.frames[Number(cursor)],
        cursor: String(Number(cursor) + 1),
        done: !session.turn && session.seedRows === 0,
      };
    }
    const messages = [];
    if (session.seedRows > 0 || session.frames.length === 0) {
      const count = Math.min(session.seedRows, chunkSize);
      for (let i = 0; i < count; i++) {
        messages.push(session.next % 2
          ? tool(session, "Historic command")
          : row(session, "assistant", "Historic fixture response. ".repeat(20)));
      }
      session.seedRows -= count;
      if (session.seedRows === 0) messages.push(row(session, "assistant", `fixture-ready-${id}`));
    } else if (session.turn) {
      const turn = session.turn;
      if (turn.index === 0) messages.push(row(session, "user", turn.text));
      messages.push(tool(session, "Streaming command"), row(session, "assistant", "Fixture response. ".repeat(12)));
      if (++turn.index === session.batches) {
        messages.push(row(session, "assistant", `${turn.text}-done`));
        session.turn = null;
      }
    }
    if (messages.length) session.frames.push(messages);
    return {
      messages,
      cursor: String(session.frames.length),
      done: !session.turn && session.seedRows === 0,
    };
  },

  closeSession(id) {
    sessions.delete(id);
    return { sessionId: id, processExited: true };
  },

  listModels() { return [{ id: "fixture", displayName: "Fixture" }]; },
};
