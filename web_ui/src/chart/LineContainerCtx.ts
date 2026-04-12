import { createContext } from "react";

export const LineContainerCtx = createContext<{
    panelMouseY: number | null;
}>({ panelMouseY: null });
