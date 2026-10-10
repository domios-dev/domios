import "./mocks/setup";
import "./mocks/theme";
import { StrictMode, useEffect, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { GalleryDetail, GalleryIndex } from "./gallery/Gallery";
import "../../src/styles/globals.css";
import "./portal.css";

function readRoute(): string {
  return window.location.hash.replace(/^#/, "");
}

function App() {
  const [route, setRoute] = useState(readRoute);
  useEffect(() => {
    const onHash = () => setRoute(readRoute());
    window.addEventListener("hashchange", onHash);
    return () => window.removeEventListener("hashchange", onHash);
  }, []);

  const detail = route.startsWith("gallery/") ? route.slice("gallery/".length) : "";
  return (
    <div className="portal">
      {detail ? <GalleryDetail id={detail} /> : <GalleryIndex />}
    </div>
  );
}

const container = document.getElementById("root") as (HTMLElement & { __galleryRoot?: Root }) | null;
if (!container) throw new Error("Playground root is missing");

container.__galleryRoot ??= createRoot(container);
container.__galleryRoot.render(
  <StrictMode>
    <App />
  </StrictMode>,
);
