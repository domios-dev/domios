// Compile-time DEV flag from vite.config.ts; release bundles define it false.
export const DEV_BUILD: boolean = typeof __CODETERM_DEV__ !== "undefined" && __CODETERM_DEV__;
