import type { IndexId } from "./types";

/** Lightweight summary returned by GET /strategies */
export interface Strategy {
    id: string;
    name: string;
    isActive?: boolean;
}

/** Full strategy with scripts/indicators, returned by GET /strategies/{id} */
export interface StrategyDetail extends Strategy {
    onIdle: string;
    onOpen: string;
    onBusy: string;
    indicators: IndexId[];
}
