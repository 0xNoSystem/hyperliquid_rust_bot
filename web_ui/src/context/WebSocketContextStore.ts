import { createContext, useContext } from "react";
import type { AddMarketInfo, MarketInfo, assetMeta } from "../types";
import type { Strategy } from "../strats.ts";

export interface WebSocketContextValue {
    markets: MarketInfo[];
    universe: assetMeta[];
    cachedMarkets: AddMarketInfo[];
    totalMargin: number;
    errorMsg: string | null;
    isOffline: boolean;
    sendCommand: (body: unknown) => Promise<Response>;
    dismissError: () => void;
    cacheMarket: (market: MarketInfo) => void;
    deleteCachedMarket: (asset: string) => void;
    requestRemoveMarket: (asset: string) => Promise<void>;
    requestToggleMarket: (asset: string, pause: boolean) => Promise<void>;
    requestCloseAll: () => Promise<void>;
    requestPauseAll: () => Promise<void>;
    updateMarketStrategy: (asset: string, strategy: Strategy) => void;
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
