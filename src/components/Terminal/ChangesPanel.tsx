import { useRef } from "react";
import { ChevronLeft, ChevronRight, FileDiff, Info } from "lucide-react";
import type { RecordedFileChange } from "@codeterm/chat-core";
import { formatCompact, formatExact } from "../../lib/formatCompact";
import { planSidePanelStore, usePlanSidePanelEntry } from "../Canvas/planSidePanelStore";
import { Segmented } from "../shared/Segmented";
import { useChatChanges } from "./chatChangesStore";
import { changeKey, changesFilePath, changesTotals, scopedFiles, turnOptions, type ChangesScope } from "./changesViewModel";
import { ChangeDetail, askAboutChange } from "./tabPanel/ChangeDetail";
import { useHomeDirectory } from "./tabPanel/homeDirectory";
import { ChangeBar, ChatAction, CompactCount, CountPair, IconAction, PanelEmpty, PanelPage, StatusGlyph } from "./tabPanel/PanelParts";
import { PanelFileTree } from "./tabPanel/PanelFileTree";
import { buildPathTree, displayPath, treeLeaves } from "./tabPanel/pathTree";
import { resolveDetail, useTabPanelNavigation } from "./tabPanel/tabPanelNavigation";

export function ChangesButton({ tabId }: { tabId: string }) {
  const changes = useChatChanges(tabId);
  const entry = usePlanSidePanelEntry();
  if (!changes.cumulative.length) return null;
  const { additions, deletions } = changesTotals(changes.cumulative);
  const open = entry?.target.documentsTabId === tabId && entry.target.tabView === "changes";
  return <button type="button" aria-label={`Changes (+${additions} -${deletions})`} aria-expanded={open}
    title={`Changes: ${formatExact(additions)} added, ${formatExact(deletions)} removed lines`}
    className="ct-bar-pill"
    onClick={() => open ? planSidePanelStore.close() : planSidePanelStore.open({ planId: "", documentsTabId: tabId, tabView: "changes" })}>
    <FileDiff size={14} aria-hidden /><span className="text-[var(--ct-green)]">+{formatCompact(additions)}</span><span className="text-[var(--ct-red)]">-{formatCompact(deletions)}</span>
  </button>;
}

function ScopeBar({ scope, options, files, onScope }: { scope: ChangesScope; options: ReturnType<typeof turnOptions>; files: readonly RecordedFileChange[]; onScope: (scope: ChangesScope) => void }) {
  const totals = changesTotals(files);
  const index = scope === "all" ? -1 : options.findIndex((option) => option.turnId === scope.turnId);
  const step = (offset: number) => { const next = options[index + offset]; if (next) onScope({ turnId: next.turnId }); };
  return <>
    <div className="tab-panel__bar-main">
      <Segmented aria-label="Changes scope" value={scope === "all" ? "all" : "turn"} height={26}
        options={[{ value: "all", label: "All" }, { value: "turn", label: "By turn" }]}
        onChange={(value) => onScope(value === "all" ? "all" : { turnId: options[0]?.turnId ?? "" })} />
      {scope !== "all" && options.length > 0 && <span className="inline-flex items-center">
        <IconAction label="Newer turn" icon={ChevronLeft} disabled={index <= 0} onClick={() => step(-1)} />
        <select aria-label="Turn" className="tab-panel__select" value={scope.turnId} onChange={(event) => onScope({ turnId: event.target.value })}>
          {options.map((option) => <option key={option.turnId} value={option.turnId}>{option.label} ({option.detail})</option>)}
        </select>
        <IconAction label="Older turn" icon={ChevronRight} disabled={index < 0 || index >= options.length - 1} onClick={() => step(1)} />
      </span>}
    </div>
    <span role="status" className="tab-panel__hint">
      <CompactCount value={files.length} unit={files.length === 1 ? "file" : "files"} /> {files.length === 1 ? "file" : "files"}
      <CountPair additions={totals.additions} deletions={totals.deletions} />
      <span title="Only edits recorded in the loaded transcript; changes made from a shell are not tracked." className="inline-flex">
        <Info size={13} aria-hidden /><span className="sr-only">Only edits recorded in the loaded transcript; changes made from a shell are not tracked.</span>
      </span>
    </span>
  </>;
}

export function ChangesView({ tabId, cwd }: { tabId: string; cwd?: string | null }) {
  const changes = useChatChanges(tabId);
  const home = useHomeDirectory(tabId);
  const [navigation, dispatch] = useTabPanelNavigation(tabId);
  const returnTo = useRef<string | null>(null);
  const options = turnOptions(changes.turns, (ms) => new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" }));
  const requested = navigation.changes.scope;
  const scope: ChangesScope = requested === "all" || options.some((option) => option.turnId === requested.turnId) ? requested : "all";
  const files = scopedFiles(changes, scope);
  const display = (path: string) => displayPath(path, { cwd, home });
  const shown = (file: RecordedFileChange) => display(changeKey(file));
  const detail = resolveDetail(navigation.changes.detail, files.map(changeKey));
  const active = files.find((file) => changeKey(file) === detail);
  const back = () => { returnTo.current = detail; dispatch({ type: "back", view: "changes" }); };
  if (active) {
    const currentFile = changes.cumulative.find((file) => file.diffs.includes(active.diffs[0])) ?? active;
    return <ChangeDetail key={`${JSON.stringify(scope)}:${detail}`} tabId={tabId} file={active} currentFile={currentFile} turns={changes.turns} path={shown(active)} cwd={cwd} display={display} onBack={back} />;
  }
  if (!changes.cumulative.length) return <PanelPage><PanelEmpty icon={FileDiff}>File edits the agent makes in this tab appear here.</PanelEmpty></PanelPage>;
  const nodes = buildPathTree(files.map((file) => ({ id: changeKey(file), path: shown(file), item: file })));
  return <PanelPage bar={<ScopeBar scope={scope} options={options} files={files} onScope={(next) => dispatch({ type: "scope", scope: next })} />}>
    {!files.length ? <PanelEmpty icon={FileDiff}>No recorded edits in this turn.</PanelEmpty>
      : <PanelFileTree label="Changed files" nodes={nodes} current={returnTo.current} onOpen={(node) => dispatch({ type: "open", view: "changes", id: node.id })}
        folderMeta={(folder) => <CountPair {...changesTotals(treeLeaves(folder))} />}
        leaf={({ item, name }) => ({
          label: `Show changes for ${changeKey(item)}`,
          title: item.movePath ? `${item.path} → ${item.movePath}` : changesFilePath(item, cwd) ?? item.path,
          icon: <StatusGlyph kind={item.kind} />,
          name: <span className={item.kind === "delete" ? "tab-panel__deleted-name" : undefined}>{name}</span>,
          meta: <><CountPair additions={item.additions} deletions={item.deletions} /><ChangeBar additions={item.additions} deletions={item.deletions} /></>,
          actions: <ChatAction compact text="Ask" label={`Ask about the change to ${shown(item)}`} onClick={() => askAboutChange(tabId, item, shown(item), cwd)} />,
        })} />}
  </PanelPage>;
}
