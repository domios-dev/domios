import { Plus } from "lucide-react";
import { Button, type ButtonSize, type ButtonVariant } from "../../../../src/components/shared/Button";
import { ForcePseudo } from "../stageTools";
import { defineSpecimen } from "../types";

type ButtonSpecimenProps = { variant: ButtonVariant; label?: string; hover?: boolean };
const SIZES: ButtonSize[] = ["compact", "sm", "md", "lg"];

function ButtonSpecimen({ variant, label = "Run agent", hover = false }: ButtonSpecimenProps) {
  const text = variant === "icon" ? <Plus size={14} /> : label;
  const row = <div className="gallery-row" style={{ flexDirection: "column" }}>
    <div className="gallery-row">{SIZES.map((size) => <Button key={size} variant={variant} size={size} aria-label={label}>{text}</Button>)}</div>
    <div className="gallery-row">
      <Button variant={variant} disabled aria-label={label}>{text}</Button>
      <Button variant={variant} loading aria-label={label}>{text}</Button>
    </div>
  </div>;
  return hover ? <ForcePseudo selector=".ct-btn" state="hover">{row}</ForcePseudo> : row;
}

export default defineSpecimen({
  id: "liquid-buttons",
  title: "Buttons",
  category: "Buttons",
  stage: "wide",
  where: "src/components/shared/Button.tsx",
  notes: "Each variant at every size (compact, sm, md, lg), then disabled and loading. The hover stage forces the shipped :hover rule on the first button.",
  Component: ButtonSpecimen,
  states: [
    { id: "primary", label: "Primary", props: { variant: "primary" } },
    { id: "primary-hover", label: "Primary · hover", props: { variant: "primary", hover: true } },
    { id: "accent", label: "Accent", props: { variant: "accent", label: "Working" } },
    { id: "ghost", label: "Ghost", props: { variant: "ghost", label: "Details" } },
    { id: "danger", label: "Danger", props: { variant: "danger", label: "Stop" } },
    { id: "icon", label: "Icon", props: { variant: "icon", label: "Add" } },
    { id: "capsule", label: "Capsule", props: { variant: "capsule", label: "opus · high" } },
    { id: "capsule-add", label: "Capsule add", props: { variant: "capsuleAdd", label: "Add" } },
  ],
});
