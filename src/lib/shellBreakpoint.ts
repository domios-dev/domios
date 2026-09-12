/** The app shell's one responsive boundary: at or below it the sidebar overlays, above it it is in flow. */
export const SHELL_NARROW_MAX_PX = 640;

// Deliberately px: the root font size tracks the terminal font setting, so a rem boundary would drift with it.
export const SHELL_NARROW_QUERY = `(max-width: ${SHELL_NARROW_MAX_PX}px)`;

/** The exact complement of SHELL_NARROW_QUERY, so no width resolves to neither mode. */
export const SHELL_WIDE_QUERY = `not all and ${SHELL_NARROW_QUERY}`;
