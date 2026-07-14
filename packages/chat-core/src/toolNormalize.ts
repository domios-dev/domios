export interface EditPair {
  old: string;
  new: string;
}

function lineCount(value: string): number {
  return value ? value.split("\n").length : 0;
}

export function countAddedLines(edit: EditPair): number {
  return lineCount(edit.new);
}

export function countRemovedLines(edit: EditPair): number {
  return lineCount(edit.old);
}

export function countAdded(edit: EditPair): number {
  return countAddedLines(edit);
}

export function countRemoved(edit: EditPair): number {
  return countRemovedLines(edit);
}
