import {useMemo, useState} from "react";
import type {RemoteTaskCapability} from "../../../packages/plugin-sdk/src/remoteTasks";
import {InlineErrorNotice} from "../shared/ErrorNotice";
import {Button} from "../shared/Button";
import {remoteTaskCall, useRemoteTask, type RemoteTaskCall} from "./useRemoteTask";

export function RemoteTaskTab({tabId, pluginId, sessionId, capability, call: injectedCall}: {
  tabId: string; pluginId: string; sessionId: string; capability: RemoteTaskCapability; call?: RemoteTaskCall;
}) {
  const call = useMemo(() => injectedCall ?? remoteTaskCall(tabId, pluginId, sessionId), [injectedCall, tabId, pluginId, sessionId]);
  const {binding, busy, error, readUnavailable, run} = useRemoteTask(call);
  const [prompt, setPrompt] = useState("");
  const [branch, setBranch] = useState("master");
  const unavailable = capability.availability.status === "unavailable";
  const uncertain = binding !== null && binding.phase !== "bound";
  const snapshot = binding?.snapshot;
  return <section aria-label="Remote task" className="flex min-h-0 flex-1 flex-col gap-3 overflow-auto p-4">
    <div role="status" aria-live="polite">{busy ? "Loading remote task…" : uncertain ? "Creation unconfirmed" : snapshot?.status ?? "New remote task"}</div>
    {readUnavailable && !binding && <p role="status">Remote task details are unavailable. Refresh before running this task.</p>}
    {capability.availability.status === "unavailable" && <p>{capability.availability.reason}</p>}
    {error != null && <InlineErrorNotice error={error} operation="reading the remote task" />}
    {uncertain && <p>Check your Cloud task list before submitting another task. This tab will not resubmit it.</p>}
    {!binding && <form onSubmit={event => {event.preventDefault(); if (!busy && !unavailable && prompt.trim() && branch.trim()) void run("submit", {prompt, branch});}}>
      <label className="block">Branch<input aria-label="Remote branch" value={branch} onChange={event => setBranch(event.target.value)} disabled={busy || unavailable} /></label>
      <label className="block">Task<textarea aria-label="Remote task prompt" value={prompt} onChange={event => setPrompt(event.target.value)} disabled={busy || unavailable} /></label>
      <Button type="submit" disabled={busy || unavailable || error != null || !prompt.trim() || !branch.trim()}>Run remote task</Button>
    </form>}
    {snapshot?.error && <InlineErrorNotice error={snapshot.error} operation="running the remote task" />}
    {snapshot?.result && <pre className="whitespace-pre-wrap">{snapshot.result}</pre>}
    {snapshot?.diff ? <details><summary>Diff</summary><pre className="overflow-auto whitespace-pre">{snapshot.diff}</pre></details>
      : snapshot?.status === "ready" && <p>No diff was produced.</p>}
    <div className="flex gap-2">
      <Button disabled={busy || unavailable || uncertain} onClick={() => void run("refresh")}>Refresh remote task</Button>
    </div>
    {!capability.followUp && <p>Follow-up messages are unavailable. Start a new tab for another task.</p>}
    {!capability.cancel && <p>Cancellation is unavailable.</p>}
    {snapshot?.diff && <p>Apply this diff explicitly in your local checkout after review.</p>}
  </section>;
}
