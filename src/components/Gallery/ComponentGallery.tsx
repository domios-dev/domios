import React from "react";
import { Button, type ButtonVariant, type ButtonSize } from "../shared/Button";
import { Chip, type ChipVariant } from "../shared/Chip";
import { Card } from "../shared/Card";
import { FormRow } from "../shared/FormRow";
import { UI_INPUT } from "../shared/design";
import { UpdateScreen } from "../UpdateProgressLayer";
import { updateStageView, type UpdateStage } from "../../lib/updateStageView";

const BUTTON_VARIANTS: ButtonVariant[] = ["primary", "ghost", "accent", "danger", "icon"];
const BUTTON_SIZES: ButtonSize[] = ["compact", "sm", "md", "lg"];
const CHIP_VARIANTS: ChipVariant[] = ["default", "accent", "success", "danger", "warning", "muted"];

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section style={{ marginBottom: "2rem" }}>
      <h2 style={{ fontSize: "11px", textTransform: "uppercase", letterSpacing: "0.1em", opacity: 0.5, marginBottom: "0.75rem" }}>
        {title}
      </h2>
      {children}
    </section>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: "0.75rem", marginBottom: "0.5rem" }}>
      <span style={{ fontSize: "10px", opacity: 0.5, width: "7rem", flexShrink: 0 }}>{label}</span>
      {children}
    </div>
  );
}

export function isGalleryRoute(search = typeof window !== "undefined" ? window.location.search : ""): boolean {
  return new URLSearchParams(search).get("prototype") === "gallery";
}

export function ComponentGallery() {
  return (
    <div
      data-testid="component-gallery"
      style={{
        padding: "2rem",
        fontFamily: "var(--ct-font-ui, system-ui)",
        color: "var(--ct-fg, #e0e0e0)",
        background: "var(--ct-bg, #1a1a1a)",
        minHeight: "100vh",
      }}
    >
      <h1 style={{ fontSize: "1rem", fontWeight: 600, marginBottom: "2rem" }}>
        Component Gallery — shared primitives
      </h1>

      <Section title="Button — variants">
        {BUTTON_VARIANTS.map((v) => (
          <Row key={v} label={v}>
            <Button variant={v}>{v === "icon" ? "⊕" : v}</Button>
            <Button variant={v} disabled>{v === "icon" ? "⊕" : "disabled"}</Button>
            <Button variant={v} loading>{v === "icon" ? "⊕" : "loading"}</Button>
          </Row>
        ))}
      </Section>

      <Section title="Button — sizes">
        {BUTTON_SIZES.map((s) => (
          <Row key={s} label={s}>
            <Button variant="primary" size={s}>{s}</Button>
            <Button variant="ghost" size={s}>{s}</Button>
          </Row>
        ))}
      </Section>

      <Section title="Chip — variants">
        <Row label="all variants">
          {CHIP_VARIANTS.map((v) => (
            <Chip key={v} variant={v}>{v}</Chip>
          ))}
        </Row>
        <Row label="with icon">
          <Chip variant="accent">⚡ accent</Chip>
          <Chip variant="success">✓ done</Chip>
          <Chip variant="danger">✗ failed</Chip>
        </Row>
      </Section>

      <Section title="Card — variants">
        <Row label="default">
          <Card style={{ padding: "0.75rem 1rem", minWidth: "10rem" }}>
            Default card
          </Card>
        </Row>
        <Row label="soft">
          <Card variant="soft" style={{ padding: "0.75rem 1rem", minWidth: "10rem" }}>
            Soft / dashed card
          </Card>
        </Row>
        <Row label="interactive">
          <Card interactive onClick={() => {}} style={{ padding: "0.75rem 1rem", minWidth: "10rem" }}>
            Click me
          </Card>
        </Row>
      </Section>

      <Section title="FormRow — states">
        <div style={{ maxWidth: "22rem", display: "flex", flexDirection: "column", gap: "1rem" }}>
          <FormRow label="Name" htmlFor="gal-name" hint="Enter your full name">
            <input id="gal-name" className={UI_INPUT.full} placeholder="Jane Smith" />
          </FormRow>
          <FormRow label="Email" htmlFor="gal-email">
            <input id="gal-email" className={UI_INPUT.full} placeholder="jane@example.com" />
          </FormRow>
          <FormRow label="Username" htmlFor="gal-user" error="This username is already taken">
            <input id="gal-user" className={UI_INPUT.full} placeholder="janesmith" />
          </FormRow>
        </div>
      </Section>

      <Section title="Update flow — simulated">
        <UpdateFlowPreview />
      </Section>
    </div>
  );
}

const SIMULATED_TOTAL = 160 * 1024 * 1024;

function UpdateFlowPreview() {
  const [stage, setStage] = React.useState<UpdateStage | null>(null);
  const [downloaded, setDownloaded] = React.useState(0);
  const start = () => { setDownloaded(0); setStage("downloading"); };
  const advance = () => setDownloaded(value => Math.min(SIMULATED_TOTAL, value + SIMULATED_TOTAL / 4));
  return (
    <div>
      <Row label="Steps">
        <Button size="sm" variant="accent" onClick={start}>Update v1.14.10</Button>
        <Button size="sm" disabled={stage !== "downloading"} onClick={advance}>Download +25%</Button>
        <Button size="sm" disabled={!stage} onClick={() => setStage("applying")}>Install</Button>
        <Button size="sm" disabled={!stage} onClick={() => setStage("restarting")}>Restart</Button>
        <Button size="sm" disabled={!stage} onClick={() => setStage(null)}>Reset</Button>
      </Row>
      <div style={{ position: "relative", height: 420, borderRadius: 12, overflow: "hidden", border: "1px solid var(--ct-border)" }}>
        {stage
          ? <UpdateScreen contained view={updateStageView(stage, "1.14.10", downloaded, SIMULATED_TOTAL)} />
          : <div style={{ display: "grid", placeItems: "center", height: "100%", opacity: 0.5, fontSize: 12 }}>App window</div>}
      </div>
    </div>
  );
}
