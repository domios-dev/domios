import { useState, useRef, useEffect, useCallback } from "react";
import type { SearchAddon } from "@xterm/addon-search";

interface SearchBarProps {
  searchAddon: SearchAddon | null;
  onClose: () => void;
}

export function SearchBar({ searchAddon, onClose }: SearchBarProps) {
  const [query, setQuery] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  const findNext = useCallback(() => {
    if (searchAddon && query) {
      searchAddon.findNext(query, { caseSensitive: false, regex: false });
    }
  }, [searchAddon, query]);

  const findPrevious = useCallback(() => {
    if (searchAddon && query) {
      searchAddon.findPrevious(query, { caseSensitive: false, regex: false });
    }
  }, [searchAddon, query]);

  const handleInputChange = (value: string) => {
    setQuery(value);
    if (searchAddon && value) {
      searchAddon.findNext(value, { caseSensitive: false, regex: false });
    } else if (searchAddon) {
      searchAddon.clearDecorations();
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter") {
      e.preventDefault();
      if (e.shiftKey) {
        findPrevious();
      } else {
        findNext();
      }
    }
  };

  return (
    <div
      style={{
        position: "absolute",
        top: 0,
        right: 0,
        zIndex: 100,
        display: "flex",
        alignItems: "center",
        gap: 4,
        padding: "4px 8px",
        backgroundColor: "var(--ct-surface)",
        border: "1px solid var(--ct-border)",
        borderTop: "none",
        borderRadius: "0 0 6px 6px",
        boxShadow: "var(--ct-shadow-md)",
        backdropFilter: "blur(var(--ct-blur-overlay))",
        WebkitBackdropFilter: "blur(var(--ct-blur-overlay))",
      }}
    >
      <input
        ref={inputRef}
        type="text"
        value={query}
        onChange={(e) => handleInputChange(e.target.value)}
        onKeyDown={handleKeyDown}
        placeholder="Find"
        style={{
          width: 180,
          padding: "4px 8px",
          border: "1px solid var(--ct-border)",
          borderRadius: 4,
          background: "var(--ct-bg)",
          color: "var(--ct-fg)",
          fontSize: 13,
          fontFamily: "inherit",
          outline: "none",
        }}
      />
      <button
        onClick={findPrevious}
        title="Previous (Shift+Enter)"
        style={btnStyle}
      >
        &#x25B2;
      </button>
      <button
        onClick={findNext}
        title="Next (Enter)"
        style={btnStyle}
      >
        &#x25BC;
      </button>
      <button
        onClick={() => {
          searchAddon?.clearDecorations();
          onClose();
        }}
        title="Close (Escape)"
        style={btnStyle}
      >
        &#x2715;
      </button>
    </div>
  );
}

const btnStyle: React.CSSProperties = {
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  width: 24,
  height: 24,
  border: "none",
  borderRadius: 4,
  background: "none",
  color: "var(--ct-fg)",
  fontSize: 12,
  cursor: "pointer",
  padding: 0,
};
