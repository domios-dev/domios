/** A prompt question reads as its first line and the detail lines that follow, line breaks intact. */
export function promptQuestionParts(question: string): { title: string; details: string } {
  const lines = question.replace(/\r\n?/g, "\n").split("\n");
  while (lines.length && !lines[lines.length - 1].trim()) lines.pop();
  const first = lines.findIndex((line) => line.trim());
  if (first < 0) return { title: "", details: "" };
  return { title: lines[first], details: lines.slice(first + 1).join("\n") };
}
