module.exports.default = {
  subscribe({ kind, params, subscriptionId, token }) {
    if (kind !== "fixture.message") return { error: "unknown fixture event kind" };
    if (params.channel !== "fixture") return { error: "invalid fixture channel" };
    return host.io({op: "graphEventEmit", event: {
      subscriptionId,
      token,
      eventId: `fixture-${subscriptionId}`,
      payload: { message: "fixture-message" },
    }}, result => result);
  },
  unsubscribe() {
    return {};
  },
};
