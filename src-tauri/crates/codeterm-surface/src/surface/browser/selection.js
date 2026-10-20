(() => {
  const element = document.activeElement;
  let text;
  if (element && (element.tagName === "INPUT" || element.tagName === "TEXTAREA")) {
    if (element.type === "password") return "";
    if (typeof element.selectionStart === "number" && typeof element.selectionEnd === "number") {
      text = element.value.slice(element.selectionStart, element.selectionEnd);
    }
  }
  text ??= String(window.getSelection() ?? "");
  if (text.length > 1_048_576) throw new Error("Selected text exceeds the browser clipboard limit");
  return text;
})()
