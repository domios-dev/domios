/** Fades out the index.html boot cover once the first complete interface is painted. */
export function releaseBootCover(): void {
  if (typeof document === "undefined") return;
  const cover = document.getElementById("ct-boot");
  if (!cover || cover.dataset.released) return;
  cover.dataset.released = "true";
  cover.addEventListener("transitionend", () => cover.remove(), { once: true });
}
