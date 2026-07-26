import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { GalleryDetail, GalleryIndex } from "./gallery/Gallery";
import "./ds/tokens.css";
import "./playground.css";
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

const root = document.getElementById("root");
if (!root) throw new Error("Playground root is missing");

createRoot(root).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
