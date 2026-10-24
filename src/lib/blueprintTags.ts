import vocabulary from "../../src-tauri/crates/codeterm-blueprint-types/resources/blueprint-tags.json";

export const BLUEPRINT_TAGS: readonly { name: string; description: string }[] = vocabulary;
export const isBlueprintTag = (tag: string): boolean => BLUEPRINT_TAGS.some((entry) => entry.name === tag);
