export async function copyCodeText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const fallback = document.createElement("textarea");
    fallback.value = text;
    fallback.style.position = "fixed";
    fallback.style.opacity = "0";
    document.body.appendChild(fallback);
    try {
      fallback.select();
      if (!document.execCommand("copy")) throw new Error("Clipboard copy failed");
    } finally {
      fallback.remove();
    }
  }
}
