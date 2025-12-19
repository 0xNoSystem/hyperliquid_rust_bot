import React, {
    createContext,
    useCallback,
    useContext,
    useEffect,
    useRef,
    useState,
} from "react";
import type {
    editMarketInfo,
    AddMarketInfo,
    MarketInfo,
    MarketTradeInfo,
    Message,
    assetMargin,
    assetMeta,
    assetPrice,
} from "../types";
import {API_URL, WS_ENDPOINT} from "../consts";
import { market_add_info } from "../types";

const CACHED_MARKETS_KEY = "cachedMarkets.v1";
const MARKET_INFO_KEY = "markets.v1";
const UNIVERSE_KEY = "universe.v1";

interface WebSocketContextValue {
    markets: MarketInfo[];
    universe: assetMeta[];
    cachedMarkets: AddMarketInfo[];
    totalMargin: number;
    errorMsg: string | null;
    sendCommand: (body: unknown) => Promise<Response>;
    dismissError: () => void;
    cacheMarket: (market: MarketInfo) => void;
    deleteCachedMarket: (asset: string) => void;
    requestRemoveMarket: (asset: string) => Promise<void>;
    requestToggleMarket: (asset: string) => Promise<void>;
    requestCloseAll: () => Promise<void>;
    requestPauseAll: () => Promise<void>;
}

const WebSocketContext = createContext<WebSocketContextValue | undefined>(
    undefined
);

const DEFAULT_PLACEHOLDER_PARAMS: MarketInfo["params"] = {
    timeFrame: "min1",
    lev: 0,
    tradeTime: 0,
    strategy: {
        custom: {
            risk: "Normal",
            style: "Scalp",
            stance: "Neutral",
            followTrend: false,
        },
    },
};

const dedupeMarkets = (markets: MarketInfo[]): MarketInfo[] => {
    const map = new Map<string, MarketInfo>();
    for (const m of markets) map.set(m.asset, m);
    return Array.from(map.values());
};

export const WebSocketProvider: React.FC<{ children: React.ReactNode }> = ({
    children,
}) => {
    const [markets, setMarkets] = useState<MarketInfo[]>([]);
    const [universe, setUniverse] = useState<assetMeta[]>([]);
    const [cachedMarkets, setCachedMarkets] = useState<AddMarketInfo[]>([]);
    const [totalMargin, setTotalMargin] = useState(0);
    const [errorMsg, setErrorMsg] = useState<string | null>(null);

    const wsRef = useRef<WebSocket | null>(null);
    const reconnectRef = useRef<number>();
    const errorTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const hasLocalMarketsRef = useRef(false);
    const activeRef = useRef(true);

    /** ------------ util functions (stable) ------------ **/
    const sendCommand = useCallback(async (body: unknown) => {
        const res = await fetch(`${API_URL}/command`, {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify(body),
        });
        if (!res.ok) throw new Error(`Command failed: ${res.status}`);
        return res;
    }, []);

    const setErrorWithTimeout = useCallback((message: string | null) => {
        if (errorTimeoutRef.current) {
            clearTimeout(errorTimeoutRef.current);
            errorTimeoutRef.current = null;
        }
        setErrorMsg(message);
        if (message) {
            errorTimeoutRef.current = setTimeout(() => {
                setErrorMsg(null);
                errorTimeoutRef.current = null;
            }, 5000);
        }
    }, []);

    /** ------------ localStorage hydration ------------ **/
    useEffect(() => {
        try {
            const raw = localStorage.getItem(MARKET_INFO_KEY);
            if (raw) {
                const parsed = JSON.parse(raw) as MarketInfo[];
                hasLocalMarketsRef.current = parsed.length > 0;
                setMarkets(dedupeMarkets(parsed));
            }
        } catch {}
        try {
            const raw = localStorage.getItem(CACHED_MARKETS_KEY);
            if (raw) setCachedMarkets(JSON.parse(raw));
        } catch {}

        try{
            const raw = localStorage.getItem(MARKET_INFO_KEY);
            if (raw) {
                const parsed = JSON.parse(raw) as assetMeta[];
                setUniverse(parsed);
            }
        }catch{}
    }, []);

    useEffect(() => {
        localStorage.setItem(MARKET_INFO_KEY, JSON.stringify(markets));
        hasLocalMarketsRef.current = markets.length > 0;
    }, [markets]);

    useEffect(() => {
        localStorage.setItem(CACHED_MARKETS_KEY, JSON.stringify(cachedMarkets));
    }, [cachedMarkets]);

    useEffect(() => {
        localStorage.setItem(UNIVERSE_KEY, JSON.stringify(universe));
    }, [universe]);

 

    useEffect(() => {
        const onStorage = (e: StorageEvent) => {
            if (e.key === MARKET_INFO_KEY) {
                if (!e.newValue) {
                    setMarkets([]);
                    hasLocalMarketsRef.current = false;
                    return;
                }
                try {
                    const parsed = JSON.parse(e.newValue) as MarketInfo[];
                    hasLocalMarketsRef.current = parsed.length > 0;
                    setMarkets(dedupeMarkets(parsed));
                } catch {}
            }
            if (e.key === CACHED_MARKETS_KEY) {
                if (!e.newValue) {
                    setCachedMarkets([]);
                    return;
                }
                try {
                    setCachedMarkets(JSON.parse(e.newValue));
                } catch {}
            }
            if (e.key === UNIVERSE_KEY) {
                if (!e.newValue) {
                    setUniverse([]);
                    return;
                }
            try{
            const raw = localStorage.getItem(MARKET_INFO_KEY);
            if (raw) {
                const parsed = JSON.parse(raw) as assetMeta[];
                setUniverse(parsed);
            }
        }catch{}

            }
        };
        window.addEventListener("storage", onStorage);
        return () => window.removeEventListener("storage", onStorage);
    }, []);

    /** ------------ unified message handler (stable) ------------ **/
    const handleMessage = useCallback(
        (event: MessageEvent) => {
            const payload = JSON.parse(event.data) as Message;

            if ("confirmMarket" in payload) {
                const asset = payload.confirmMarket.asset;
                setMarkets((prev) => {
                    const has = prev.some((m) => m.asset === asset);
                    const updated = has
                        ? prev.map((m) =>
                              m.asset === asset
                                  ? { ...payload.confirmMarket, state: "Ready" }
                                  : m
                          )
                        : [
                              ...prev,
                              { ...payload.confirmMarket, state: "Ready" },
                          ];
                    return dedupeMarkets(updated);
                });
                return;
            }

            if ("preconfirmMarket" in payload) {
                const asset = payload.preconfirmMarket;
                setMarkets((prev) =>
                    prev.some((m) => m.asset === asset)
                        ? prev
                        : [
                              ...prev,
                              {
                                  asset,
                                  state: "Loading",
                                  price: null,
                                  prev: null,
                                  lev: null,
                                  margin: null,
                                  pnl: null,
                                  indicators: [],
                                  trades: [],
                                  params: DEFAULT_PLACEHOLDER_PARAMS,
                                  isPaused: false,
                                  position: null,
                              },
                          ]
                );
                return;
            }

            if ("marketInfoEdit" in payload) {
                const [asset, edit] = payload.marketInfoEdit as [
                    string,
                    EditMarketInfo,
                ];

                setMarkets((prev) =>
                    prev.map((m) => {
                        if (m.asset !== asset) return m;

                        if ("lev" in edit) {
                            return { ...m, lev: edit.lev };
                        }

                        if ("price" in edit) {
                            return { ...m, price: edit.price };
                        }

                        if ("openPosition" in edit) {
                            return { ...m, position: edit.openPosition };
                        }

                        if ("trade" in edit) {
                            const trades = [...(m.trades ?? []), edit.trade];
                            return {
                                ...m,
                                trades,
                                pnl: (m.pnl ?? 0) + edit.trade.pnl,
                            };
                        }

                        return m;
                    })
                );
                return;
            }

            if ("updateTotalMargin" in payload) {
                setTotalMargin(payload.updateTotalMargin);
                return;
            }

            if ("updateMarketMargin" in payload) {
                const [asset, margin] =
                    payload.updateMarketMargin as assetMargin;
                setMarkets((prev) =>
                    prev.map((m) => (m.asset === asset ? { ...m, margin } : m))
                );
                return;
            }

            if ("updateIndicatorValues" in payload) {
                const { asset, data } = payload.updateIndicatorValues;
                setMarkets((prev) =>
                    prev.map((m) =>
                        m.asset === asset ? { ...m, indicators: data } : m
                    )
                );
                return;
            }

            if ("userError" in payload) {
                setErrorWithTimeout(payload.userError);
                return;
            }

            if ("loadSession" in payload) {
                const [sessionMarkets, meta] = payload.loadSession as [
                    MarketInfo[],
                    assetMeta[],
                ];
                setUniverse(meta);
                setMarkets((prev) => {
                    if (hasLocalMarketsRef.current && prev.length > 0)
                        return prev;
                    const deduped = dedupeMarkets(sessionMarkets);
                    hasLocalMarketsRef.current = deduped.length > 0;
                    return deduped;
                });
                return;
            }
        },
        [setErrorWithTimeout]
    );

    /** ------------ socket lifecycle (runs once) ------------ **/
    useEffect(() => {
        let retry = 0;
        activeRef.current = true;

        const connect = () => {
            if (!activeRef.current) return;
            console.log("WS connect");

            const ws = new WebSocket(WS_ENDPOINT);
            wsRef.current = ws;

            const onOpen = () => {
                retry = 0;
                sendCommand({ getSession: null }).catch(console.error);
            };
            const onClose = () => {
                if (!activeRef.current) return;
                const delay = Math.min(1000 * 2 ** retry, 15000);
                retry++;
                reconnectRef.current = window.setTimeout(connect, delay);
            };

            ws.addEventListener("open", onOpen);
            ws.addEventListener("message", handleMessage);
            ws.addEventListener("error", console.error);
            ws.addEventListener("close", onClose);
        };

        connect();

        return () => {
            activeRef.current = false;
            if (reconnectRef.current) clearTimeout(reconnectRef.current);
            const ws = wsRef.current;
            if (ws) {
                ws.removeEventListener("message", handleMessage);
                ws.close();
            }
            wsRef.current = null;
            if (errorTimeoutRef.current) clearTimeout(errorTimeoutRef.current);
        };
    }, []); // important: run once

    /** ------------ exposed API ------------ **/
    const cacheMarket = useCallback((market: MarketInfo) => {
        setCachedMarkets((prev) => {
            const cached = market_add_info(market);
            return prev.some((m) => m.asset === cached.asset)
                ? prev
                : [...prev, cached];
        });
    }, []);

    const deleteCachedMarket = useCallback(
        (asset: string) =>
            setCachedMarkets((p) => p.filter((m) => m.asset !== asset)),
        []
    );

    const requestRemoveMarket = useCallback(
        async (asset: string) => {
            await sendCommand({ removeMarket: asset });
            setMarkets((p) => p.filter((m) => m.asset !== asset));
        },
        [sendCommand]
    );

    const requestToggleMarket = useCallback(
        async (asset: string, pause: boolean) => {
            await sendCommand(
                pause ? { pauseMarket: asset } : { resumeMarket: asset }
            );
            setMarkets((p) =>
                p.map((m) =>
                    m.asset === asset ? { ...m, isPaused: !m.isPaused } : m
                )
            );
        },
        [sendCommand]
    );

    const requestCloseAll = useCallback(async () => {
        await sendCommand({ closeAll: null });
        setMarkets([]);
    }, [sendCommand]);

    const requestPauseAll = useCallback(async () => {
        await sendCommand({ pauseAll: null });
        setMarkets((p) => p.map((m) => ({ ...m, isPaused: true })));
    }, [sendCommand]);

    const dismissError = useCallback(
        () => setErrorWithTimeout(null),
        [setErrorWithTimeout]
    );

    const value: WebSocketContextValue = {
        markets,
        universe,
        cachedMarkets,
        totalMargin,
        errorMsg,
        sendCommand,
        dismissError,
        cacheMarket,
        deleteCachedMarket,
        requestRemoveMarket,
        requestToggleMarket,
        requestCloseAll,
        requestPauseAll,
    };

    return (
        <WebSocketContext.Provider value={value}>
            {children}
        </WebSocketContext.Provider>
    );
};

export const useWebSocketContext = (): WebSocketContextValue => {
    const ctx = useContext(WebSocketContext);
    if (!ctx)
        throw new Error(
            "useWebSocketContext must be used within WebSocketProvider"
        );
    return ctx;
};
