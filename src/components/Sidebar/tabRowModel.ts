export interface TabRowTextInput {
  title: string;
  description?: string | null;
  typeLabel?: string | null;
}

export interface TabRowText {
  title: string;
  description: string | null;
  typeLabel: string | null;
}

export function tabRowText({ title, description, typeLabel }: TabRowTextInput): TabRowText {
  return {
    title,
    description: description?.trim() || null,
    typeLabel: typeLabel?.trim() || null,
  };
}
