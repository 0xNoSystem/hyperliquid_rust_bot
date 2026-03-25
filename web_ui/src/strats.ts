import type { IndexId } from "./types";

export interface Strategy {
    id: string;
    name: string;
    onIdle: string;
    onOpen: string;
    onBusy: string;
    indicators: IndexId[];
    isActive?: boolean;
}
