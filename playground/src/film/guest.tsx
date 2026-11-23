import { createContext, useContext } from "react";

/** The viewer's own machine, named by the page ("Your Mac"), when it joins the film's mesh; null in videos. */
const GuestContext = createContext<string | null>(null);
export const GuestProvider = GuestContext.Provider;
export const useGuest = () => useContext(GuestContext);

/** The moment in mesh-hero the viewer's machine joins: as "one private mesh" lands. */
export const GUEST_JOINS = 5.2;
