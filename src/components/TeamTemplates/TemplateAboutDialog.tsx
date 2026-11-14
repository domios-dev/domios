import { useId, useState } from "react";
import { X } from "lucide-react";
import type { TeamDocument } from "../../../gen/TeamDocument";
import type { ProviderDescriptor } from "../../lib/types";
import type { TemplatePreview } from "../../lib/transport/teamTemplates";
import { managerRoles, templateTitle } from "../../lib/teamTemplates/startModel";
import { FormDialog } from "../shared/FormDialog";
import { DropdownSelect } from "../shared/DropdownMenu";
import { TemplateIdentity } from "./TemplateIdentity";
import { TemplateBrings } from "./TemplateBrings";
import { TemplateAsks } from "./TemplateAsks";
import { TemplateInstructions } from "./TemplateInstructions";
import { WorkerChips } from "./WorkerChips";
import { TemplateRoleList } from "./TemplateRoleList";
import { ReadInstructionsButton } from "./ReadInstructionsButton";
import { TemplateMarkdown, TemplatePackageText, TemplateYaml } from "./TemplatePackageText";
import { useTemplateDocument } from "./useTemplateDocument";
import type { TeamTemplateChoice } from "./useTeamTemplateChoice";
import "./teamTemplates.css";

const SECTIONS = [
  { value: "overview", label: "Overview" },
  { value: "roles", label: "Roles" },
  { value: "files", label: "Files and docs" },
  { value: "permissions", label: "Permissions and secrets" },
  { value: "yaml", label: "team.yaml" },
] as const;
type AboutSection = typeof SECTIONS[number]["value"];

function PackageFiles({ preview, document }: { preview: TemplatePreview; document: TeamDocument }) {
  const files = document.files ?? [];
  const [path, setPath] = useState(files[0]?.path ?? "");
  if (!files.length) return <p>No package files.</p>;
  const current = files.find((file) => file.path === path) ?? files[0];
  const markdown = /\.(md|markdown)$/i.test(current.path);
  return <div className="flex min-w-0 flex-col gap-4">
    <DropdownSelect aria-label="Package file" value={current.path} onChange={setPath} focusOnPointerOpen options={files.map((file) => ({ value: file.path, label: file.path }))} />
    {current.description && <p className="text-[var(--ct-muted-fg)]">{current.description}</p>}
    {current.content != null
      ? markdown ? <TemplateMarkdown text={current.content} /> : <pre className="whitespace-pre-wrap break-words">{current.content}</pre>
      : <TemplatePackageText key={current.path} template={preview.form.template} target={{ path: current.path }} markdown={markdown} />}
  </div>;
}

export function TemplateAboutDialog({ choice, preview, providers, initialSection = "overview", onClose }: {
  choice: TeamTemplateChoice;
  preview: TemplatePreview;
  providers: readonly ProviderDescriptor[];
  initialSection?: AboutSection;
  onClose: () => void;
}) {
  const titleId = useId();
  const [section, setSection] = useState<AboutSection>(initialSection);
  const { result, error } = useTemplateDocument(preview.form.template);
  return <FormDialog titleId={titleId} onCancel={onClose} onSubmit={(event) => event.preventDefault()} fullscreen stacked className="template-about">
    <header className="flex shrink-0 items-center justify-between gap-4 border-b border-[var(--ct-border)] pb-3">
      <h2 id={titleId} className="text-base font-semibold">About {templateTitle(preview.form.template)}</h2>
      <button type="button" aria-label="Close team details" className="ct-small-pill p-2" onClick={onClose}><X size={18} /></button>
    </header>
    <div className="template-about-layout min-h-0 flex-1">
      <div className="template-about-mobile"><DropdownSelect aria-label="Team details section" value={section} options={[...SECTIONS]} focusOnPointerOpen onChange={(value) => setSection(value as AboutSection)} /></div>
      <nav aria-label="Team details sections" className="template-about-nav flex-col gap-1">
        {SECTIONS.map((item) => <button key={item.value} type="button" aria-current={section === item.value ? "page" : undefined}
          className={`rounded-lg px-3 py-2 text-left text-[13px] ${section === item.value ? "bg-[var(--ct-bg)] text-[var(--ct-fg)]" : "text-[var(--ct-muted-fg)] hover:bg-[var(--ct-bg)]"}`}
          onClick={() => setSection(item.value)}>{item.label}</button>)}
      </nav>
      <section key={section} aria-label={SECTIONS.find((item) => item.value === section)?.label} className="min-h-0 min-w-0 overflow-y-auto overscroll-contain px-1 pb-6 sm:px-6">
        <div className="mx-auto flex max-w-4xl flex-col gap-5 text-[13px]">
          {section === "overview" && <>
            <TemplateIdentity summary={preview.form.template} />
            <TemplateRoleList title="Managers" roles={managerRoles(preview.form.roles)} preview={preview} providers={providers} />
            <WorkerChips preview={preview} />
            <ReadInstructionsButton preview={preview} onClick={() => setSection("roles")} />
            {!!preview.form.template.when?.length && <div><h3 className="mb-2 font-medium">When to use</h3><ul className="list-disc pl-5">{preview.form.template.when.map((when) => <li key={when}>{when}</li>)}</ul></div>}
            <TemplateBrings payload={preview.payload ?? []} optOut={choice.optOut} onToggle={choice.setKept} collapsible={false} template={preview.form.template} roles={preview.form.roles} />
          </>}
          {section === "roles" && <TemplateInstructions preview={preview} document={result?.document} providers={providers} />}
          {section === "permissions" && <>
            <h3 className="font-medium">Permissions and secrets</h3>
            {(preview.asks?.length ?? 0) > 0 ? <TemplateAsks asks={preview.asks ?? []} /> : <p>No permissions or secrets requested.</p>}
          </>}
          {(section === "files" || section === "yaml") && (error ? <p role="alert">{error}</p>
            : !result ? <p aria-busy="true" className="text-[var(--ct-muted-fg)]">Reading the package…</p>
              : section === "yaml" ? <TemplateYaml text={result.text} /> : <PackageFiles preview={preview} document={result.document} />)}
          {section === "roles" && error && <p role="alert">{error}</p>}
        </div>
      </section>
    </div>
  </FormDialog>;
}
