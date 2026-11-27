import {useEffect,useRef,useState} from "react";
import {invoke} from "../../lib/bridge";
import {nativeBridge} from "../../lib/nativeBridge";
import {parseCompositeTabId} from "../../lib/remoteTabTransport";
import type {AgentRuntimeState} from "../../lib/agentRuntimeState";
import {runtimeLoginErrorMessage} from "./runtimeLoginError";

export type CodeSubmissionUiState = {
  loginId: string;
  status: "submitted" | "accepted" | "completed" | "uncertain" | "refused";
  reason?: string;
};

export type RuntimeLoginReply = {
  login?: AgentRuntimeState["login"];
  state?: Partial<AgentRuntimeState>;
  authentication?: AgentRuntimeState["authentication"];
  apiKeyMethods?: AgentRuntimeState["apiKeyMethods"];
  loginBinding?: AgentRuntimeState["loginBinding"];
  codeSubmission?: AgentRuntimeState["codeSubmission"];
  outcome?: "accepted" | "completed" | "refused" | "uncertain";
  reason?: string;
};

const UNCERTAIN_CODE_REASON = "Sign-in code delivery is uncertain and was not replayed. Wait for sign-in to complete or cancel and start a new sign-in.";

function isProvenPreWriteRefusal(failure: unknown): boolean {
  if (typeof failure !== "object" || failure === null) return false;
  const record = failure as { bytesWritten?: unknown; status?: unknown; body?: unknown };
  if (record.bytesWritten === false) return true;
  if (typeof record.body === "string") {
    try {
      const parsed = JSON.parse(record.body) as { outcome?: unknown; error?: { bytes_written?: unknown } };
      if (parsed.outcome === "refused" || parsed.error?.bytes_written === false) return true;
    } catch { /* plain text refusal below */ }
  }
  return record.bytesWritten === undefined
    && typeof record.status === "number"
    && (record.status === 400 || record.status === 401 || record.status === 403 || record.status === 409);
}

function buildCodeBinding(tabId: string, providerId: string | undefined, state: AgentRuntimeState | undefined) {
  const remote = parseCompositeTabId(tabId);
  if (!state?.loginBinding && !remote) return undefined;
  const base = state?.loginBinding;
  if (!remote) return base;
  return {
    nonce: base?.nonce ?? "",
    epoch: base?.epoch ?? state?.epoch ?? 0,
    installationId: base?.installationId ?? remote.peerId,
    tabId: base?.tabId ?? remote.remoteTabId,
    ...(base?.providerId || providerId ? { providerId: base?.providerId ?? providerId } : {}),
    ...(base?.sessionId ? { sessionId: base.sessionId } : {}),
  };
}

export function useRuntimeLogin(
  tabId:string,
  providerId?:string,
  state?:AgentRuntimeState,
  onResult?:(reply:RuntimeLoginReply)=>void,
) {
  const [busy,setBusy]=useState(false);
  const [error,setError]=useState<string|null>(null);
  const [codeState,setCodeState]=useState<CodeSubmissionUiState|null>(null);
  const pending=useRef(false);
  const cancelPending=useRef(false);
  const cycle=useRef(0);
  const current=useRef(false);
  const submittedLoginIds=useRef(new Set<string>());
  useEffect(()=>{current.current=true;return ()=>{current.current=false;};},[]);
  async function operate(request:{action?:string;method?:string;methodId?:string;apiKey?:string;account?:string;code?:string;ownerConfirmed?:boolean}) {
    const cancel=request.action === "cancel";
    const isCode=request.action === "code";
    const loginId=state?.login?.loginId;
    if (isCode && loginId) {
      const lockedByServer = state?.codeSubmission?.loginId === loginId
        && (state.codeSubmission.status === "submitted"
          || state.codeSubmission.status === "accepted"
          || state.codeSubmission.status === "completed"
          || state.codeSubmission.status === "uncertain");
      if (submittedLoginIds.current.has(loginId) || lockedByServer) return false;
    }
    if (cancel ? cancelPending.current : pending.current) return false;
    if (cancel) { cancelPending.current=true;cycle.current++; } else pending.current=true;
    const ownCycle=cycle.current;
    const owns=()=>current.current && ownCycle===cycle.current;
    setBusy(true);setError(null);
    if (isCode && loginId) {
      submittedLoginIds.current.add(loginId);
      setCodeState({ loginId, status: "submitted" });
    }
    if (cancel) {
      if (loginId) submittedLoginIds.current.delete(loginId);
      setCodeState(null);
    }
    try {
      const binding = isCode ? buildCodeBinding(tabId, providerId, state) : state?.loginBinding;
      const result=await invoke<RuntimeLoginReply>("agent_runtime_login",{tabId,request:{...request,...(providerId ? {providerId} : {}),
        ...(binding ? {binding} : {}),
        ...(request.action && loginId ? {loginId} : {})}});
      if (owns()) onResult?.(result);
      if (owns() && isCode && loginId) {
        const secret = request.code;
        const sanitize = (text?: string) => text && secret ? text.replaceAll(secret, "<redacted>") : text;
        if (result.outcome === "uncertain") {
          setCodeState({ loginId, status: "uncertain", reason: sanitize(result.reason) ?? UNCERTAIN_CODE_REASON });
        } else if (result.outcome === "completed" || result.login?.status === "completed") {
          setCodeState({ loginId, status: "completed" });
        } else if (result.outcome === "refused") {
          const reason = sanitize(result.reason) ?? "Sign-in code refused.";
          submittedLoginIds.current.delete(loginId);
          setCodeState({ loginId, status: "refused", reason });
          setError(reason);
          return false;
        } else {
          setCodeState({ loginId, status: "accepted" });
        }
      }
      if (owns() && !request.action && (request.method === "browser" || request.methodId) && result.login?.url && !tabId.startsWith("mesh:")) await nativeBridge.openUrl(result.login.url);
      return owns();
    } catch (failure) {if(owns()) {
      const secret=request.apiKey ?? request.code;
      const message=runtimeLoginErrorMessage(failure,request.action);
      const safe=secret ? message.replaceAll(secret,"<redacted>") : message;
      if (isCode && loginId) {
        if (isProvenPreWriteRefusal(failure)) {
          submittedLoginIds.current.delete(loginId);
          setCodeState({ loginId, status: "refused", reason: safe });
          setError(safe);
        } else {
          setCodeState({ loginId, status: "uncertain", reason: UNCERTAIN_CODE_REASON });
        }
      } else {
        setError(safe);
      }
    } return false;}
    finally {if(cancel)cancelPending.current=false;else pending.current=false;if(current.current)setBusy(pending.current||cancelPending.current);}
  }
  return {busy,error,codeState,operate};
}
