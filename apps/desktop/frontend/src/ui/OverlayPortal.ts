import { createContext } from "react";

// Editors rendered in a modal keep their nested controls inside its focus boundary.
export const OverlayPortal = createContext<HTMLElement | null>(null);
