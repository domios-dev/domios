import type { ComponentType } from "react";

export type SpecimenState = {
  id: string;
  label: string;
  props: Record<string, unknown>;
};

export type Specimen = {
  id: string;
  title: string;
  category: string;
  where: string;
  states: SpecimenState[];
  Component: ComponentType<Record<string, unknown>>;
  notes?: string;
};
