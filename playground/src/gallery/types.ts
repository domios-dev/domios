import { createElement, type ComponentType, type ReactNode } from "react";

export type SpecimenState = {
  id: string;
  label: string;
  render: () => ReactNode;
};

export type Specimen = {
  id: string;
  title: string;
  category: string;
  where: string;
  states: SpecimenState[];
  motion?: "animated" | "static";
  notes?: string;
};

type SpecimenDefinition<Props extends object> = Omit<Specimen, "states"> & {
  states: Array<Omit<SpecimenState, "render"> & { props: Props }>;
  Component: ComponentType<Props>;
};

export function defineSpecimen<Props extends object>({ Component, states, ...specimen }: SpecimenDefinition<Props>): Specimen {
  return {
    ...specimen,
    states: states.map(({ id, label, props }) => ({
      id,
      label,
      render: () => createElement(Component, props),
    })),
  };
}
