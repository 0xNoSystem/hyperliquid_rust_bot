import React, { useCallback, useEffect, useRef, useState } from "react";
import type { AddMarketInfo, MarketInfo, Message, assetMeta } from "../types";
import type { Strategy } from "../strats";
import { API_URL, WS_ENDPOINT } from "../consts";
import { market_add_info } from "../types";
import type { WebSocketContextValue } from "./WebSocketContextStore";
import { WebSocketContext } from "./WebSocketContextStore";

const CACHED_MARKETS_KEY = "cachedMarkets.v1";
const MARKET_INFO_KEY = "markets.v1";
const UNIVERSE_KEY = "universe.v1";

const DEFAULT_PLACEHOLDER_PARAMS: MarketInfo["params"] = {
    lev: 1,
    strategy: "rsiEmaScalp",
};

const dedupeMarkets = (markets: MarketInfo[]): MarketInfo[] => {
    const map = new Map<string, MarketInfo>();
    for (const m of markets) map.set(m.asset, m);
    return Array.from(map.values());
};

const isAssetMeta = (value: unknown): value is assetMeta =>
    typeof value === "object" &&
    value !== null &&
    "name" in value &&
    "szDecimals" in value &&
    "maxLeverage" in value;

const isAssetMetaArray = (value: unknown): value is assetMeta[] =>
    Array.isArray(value) && (value.length === 0 || isAssetMeta(value[0]));

export const WebSocketProvider: React.FC<{ children: React.ReactNode }> = ({
    children,
}) => {
    const [markets, setMarkets] = useState<MarketInfo[]>([]);
    const [universe, setUniverse] = useState<assetMeta[]>([]);
    const [cachedMarkets, setCachedMarkets] = useState<AddMarketInfo[]>([]);
    const [totalMargin, setTotalMargin] = useState(0);
    const [errorMsg, setErrorMsg] = useState<string | null>(null);
    const [isOffline, setIsOffline] = useState(false);

    /** ---------- refs for latest state (CRITICAL) ---------- **/
    const marketsRef = useRef<MarketInfo[]>([]);
    const universeRef = useRef<assetMeta[]>([]);

    useEffect(() => {
        marketsRef.current = markets;
    }, [markets]);

    useEffect(() => {
        universeRef.current = universe;
    }, [universe]);

    /** ---------- infra refs ---------- **/
    const wsRef = useRef<WebSocket | null>(null);
    const reconnectRef = useRef<number | null>(null);
    const errorTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
    const hasLocalMarketsRef = useRef(false);
    const activeRef = useRef(true);

    /** ---------- utils ---------- **/
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

    /** ---------- localStorage hydration ---------- **/
    useEffect(() => {
        try {
            const raw = localStorage.getItem(MARKET_INFO_KEY);
            if (raw) {
                const parsed = JSON.parse(raw) as MarketInfo[];
                hasLocalMarketsRef.current = parsed.length > 0;
                setMarkets(dedupeMarkets(parsed));
            }
        } catch {
            console.log("Failed to hydrate localStorage");
        }

        try {
            const raw = localStorage.getItem(CACHED_MARKETS_KEY);
            if (raw) setCachedMarkets(JSON.parse(raw));
        } catch {
            console.log("Failed to hydrate localStorage");
        }

        try {
            const raw = localStorage.getItem(UNIVERSE_KEY);
            if (raw) setUniverse(JSON.parse(raw));
        } catch {
            console.log("Failed to hydrate localStorage");
        }
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

    /** ---------- WS message handler (STABLE) ---------- **/
    const handleMessage = useCallback(
        (event: MessageEvent) => {
            const payload = JSON.parse(event.data) as Message;

            if ("status" in payload) {
                if (
                    payload.status === "offline" ||
                    payload.status === "shutdown"
                ) {
                    setIsOffline(true);
                }

                if (payload.status === "shutdown") {
                    setCachedMarkets((prev) => {
                        const next = [...prev];
                        marketsRef.current.forEach((market) => {
                            const cached = market_add_info(market);
                            if (!next.some((m) => m.asset === cached.asset)) {
                                next.push(cached);
                            }
                        });
                        return next;
                    });
                    setMarkets([]);
                }
                return;
            }

            if ("confirmMarket" in payload) {
                const readyMarket: MarketInfo = {
                    ...payload.confirmMarket,
                    state: "Ready",
                };
                setMarkets((prev) =>
                    dedupeMarkets(
                        prev.some((m) => m.asset === readyMarket.asset)
                            ? prev.map((m) =>
                                  m.asset === readyMarket.asset
                                      ? readyMarket
                                      : m
                              )
                            : [...prev, readyMarket]
                    )
                );
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

            if ("cancelMarket" in payload) {
                const asset = payload.cancelMarket;
                setMarkets((p) => p.filter((m) => m.asset !== asset));
            }

            if ("marketInfoEdit" in payload) {
                const [asset, edit] = payload.marketInfoEdit;
                setIsOffline(false);
                setMarkets((prev) =>
                    prev.map((m) => {
                        if (m.asset !== asset) return m;
                        if ("lev" in edit) return { ...m, lev: edit.lev };
                        if ("price" in edit) return { ...m, price: edit.price };
                        if ("openPosition" in edit)
                            return { ...m, position: edit.openPosition };
                        if ("trade" in edit)
                            return {
                                ...m,
                                trades: [...(m.trades ?? []), edit.trade],
                                pnl: (m.pnl ?? 0) + edit.trade.pnl,
                            };
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
                const [asset, margin] = payload.updateMarketMargin;
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
                const session = payload.loadSession;
                if (isAssetMetaArray(session)) {
                    setUniverse(session);
                    return;
                }
                const [sessionMarkets, meta] = session;
                setUniverse(meta);
                setMarkets((prev) => {
                    if (hasLocalMarketsRef.current && prev.length > 0)
                        return prev;
                    const deduped = dedupeMarkets(sessionMarkets);
                    hasLocalMarketsRef.current = deduped.length > 0;
                    return deduped;
                });
            }
        },
        [setErrorWithTimeout]
    );

    /** ---------- WS lifecycle ---------- **/
    useEffect(() => {
        let retry = 0;
        activeRef.current = true;

        const connect = () => {
            if (!activeRef.current) return;

            const ws = new WebSocket(WS_ENDPOINT);
            wsRef.current = ws;

            ws.addEventListener("open", () => {
                retry = 0;
                if (universeRef.current.length === 0) {
                    sendCommand({ getSession: null }).catch(console.error);
                }
            });

            ws.addEventListener("message", handleMessage);
            ws.addEventListener("close", () => {
                if (!activeRef.current) return;
                const delay = Math.min(1000 * 2 ** retry, 15000);
                retry++;
                reconnectRef.current = window.setTimeout(connect, delay);
            });

            ws.addEventListener("error", console.error);
        };

        connect();

        return () => {
            activeRef.current = false;
            if (reconnectRef.current) clearTimeout(reconnectRef.current);
            wsRef.current?.close();
            if (errorTimeoutRef.current) clearTimeout(errorTimeoutRef.current);
        };
    }, [handleMessage, sendCommand]);

    /** ---------- API ---------- **/
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

    const updateMarketStrategy = useCallback(
        (asset: string, strategy: Strategy) => {
            setMarkets((prev) =>
                prev.map((m) =>
                    m.asset === asset
                        ? { ...m, params: { ...m.params, strategy } }
                        : m
                )
            );
        },
        []
    );

    const dismissError = useCallback(
        () => setErrorWithTimeout(null),
        [setErrorWithTimeout]
    );

    const value: WebSocketContextValue = {
        markets,
        universe,
        cachedMarkets,
        totalMargin,
        isOffline,
        errorMsg,
        sendCommand,
        dismissError,
        cacheMarket,
        deleteCachedMarket,
        requestRemoveMarket,
        requestToggleMarket,
        requestCloseAll,
        requestPauseAll,
        updateMarketStrategy,
    };

    return (
        <WebSocketContext.Provider value={value}>
            {children}
        </WebSocketContext.Provider>
    );
};
