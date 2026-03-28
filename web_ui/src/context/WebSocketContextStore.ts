import { createContext, useContext } from "react";
import type { BacktestRunState, MarketInfo, assetMeta } from "../types";
import type { Strategy } from "../strats";

export interface WebSocketContextValue {
    markets: MarketInfo[];
    universe: assetMeta[];
    cachedMarkets: string[];
    backtestRuns: Record<string, BacktestRunState>;
    strategies: Strategy[];
    totalMargin: number;
    errorMsg: string | null;
    isOffline: boolean;
    needsApiKey: boolean;
    setNeedsApiKey: (v: boolean) => void;
    sendCommand: (body: unknown) => Promise<Response>;
    dismissError: () => void;
    cacheMarket: (asset: string) => void;
    deleteCachedMarket: (asset: string) => void;
    requestRemoveMarket: (asset: string) => Promise<void>;
    requestToggleMarket: (asset: string, pause: boolean) => Promise<void>;
    requestCloseAll: () => Promise<void>;
    requestPauseAll: () => Promise<void>;
    requestSyncMargin: () => Promise<void>;
    fetchStrategies: () => Promise<void>;
    updateMarketStrategy: (asset: string, strategyName: string) => void;
}

export const WebSocketContext = createContext<
    WebSocketContextValue | undefined
>(undefined);

export const useWebSocketContext = (): WebSocketContextValue => {
    const ctx = useContext(WebSocketContext);
    if (!ctx)
        throw new Error(
            "useWebSocketContext must be used within WebSocketProvider"
        );
    return ctx;
};
