import React, { useCallback, useEffect, useRef, useState } from "react";
import type {
    BacktestRunState,
    BackendMarketInfo,
    MarketInfo,
    Message,
    assetMeta,
} from "../types";
import type { Strategy } from "../strats";
import { API_URL, WS_ENDPOINT } from "../consts";
import type { WebSocketContextValue } from "./WebSocketContextStore";
import { WebSocketContext } from "./WebSocketContextStore";
import { useAuth } from "./AuthContextStore";

const UNIVERSE_KEY = "universe.v1";
const MAX_MARKET_LOG_ENTRIES = 200;
const userKey = (base: string, addr: string | null) =>
    addr ? `${base}.${addr.toLowerCase()}` : base;

const withMarketDefaults = (market: MarketInfo): MarketInfo => ({
    ...market,
    indicators: market.indicators ?? [],
    trades: market.trades ?? [],
    log: market.log ?? [],
});

const toMarketInfo = (market: BackendMarketInfo): MarketInfo => ({
    ...market,
    state: "Ready",
    prev: market.price,
    trades: [],
    log: [],
});

const dedupeMarkets = (markets: MarketInfo[]): MarketInfo[] => {
    const map = new Map<string, MarketInfo>();
    for (const m of markets) map.set(m.asset, withMarketDefaults(m));
    return Array.from(map.values());
};

export const WebSocketProvider: React.FC<{ children: React.ReactNode }> = ({
    children,
}) => {
    const { token, address } = useAuth();
    const [markets, setMarkets] = useState<MarketInfo[]>([]);
    const [universe, setUniverse] = useState<assetMeta[]>([]);
    const [cachedMarkets, setCachedMarkets] = useState<string[]>([]);
    const [backtestRuns, setBacktestRuns] = useState<
        Record<string, BacktestRunState>
    >({});
    const [totalMargin, setTotalMargin] = useState(0);
    const [errorMsg, setErrorMsg] = useState<string | null>(null);
    const [isOffline, setIsOffline] = useState(false);
    const [needsApiKey, setNeedsApiKey] = useState(false);
    const [needsBuilderApproval, setNeedsBuilderApproval] = useState(false);
    const [strategies, setStrategies] = useState<Strategy[]>([]);

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
    /** ---------- utils ---------- **/
    const tokenRef = useRef(token);
    useEffect(() => {
        tokenRef.current = token;
    }, [token]);

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

    const sendCommand = useCallback(
        async (body: unknown) => {
            const headers: Record<string, string> = {
                "Content-Type": "application/json",
            };
            if (tokenRef.current) {
                headers["Authorization"] = `Bearer ${tokenRef.current}`;
            }
            const res = await fetch(`${API_URL}/command`, {
                method: "POST",
                headers,
                body: JSON.stringify(body),
            });
            if (res.status === 412) {
                const text = await res.text();
                const msg = text.includes("no API key")
                    ? "No API key configured. Go to Settings to add your Hyperliquid API key."
                    : text || "Precondition failed";
                setNeedsApiKey(true);
                setErrorWithTimeout(msg);
                throw new Error(msg);
            }
            if (!res.ok) throw new Error(`Command failed: ${res.status}`);
            return res;
        },
        [setErrorWithTimeout]
    );

    /** ---------- localStorage hydration (scoped per wallet) ---------- **/
    useEffect(() => {
        try {
            const raw = localStorage.getItem(userKey("markets.v1", address));
            if (raw) {
                const parsed = JSON.parse(raw) as MarketInfo[];
                hasLocalMarketsRef.current = parsed.length > 0;
                setMarkets(dedupeMarkets(parsed));
            } else {
                setMarkets([]);
                hasLocalMarketsRef.current = false;
            }
        } catch {
            console.log("Failed to hydrate localStorage");
        }

        try {
            const raw = localStorage.getItem(
                userKey("cachedMarkets.v1", address)
            );
            if (raw) setCachedMarkets(JSON.parse(raw));
            else setCachedMarkets([]);
        } catch {
            console.log("Failed to hydrate localStorage");
        }

        try {
            const raw = localStorage.getItem(UNIVERSE_KEY);
            if (raw) setUniverse(JSON.parse(raw));
        } catch {
            console.log("Failed to hydrate localStorage");
        }
    }, [address]);

    useEffect(() => {
        if (address)
            localStorage.setItem(
                userKey("markets.v1", address),
                JSON.stringify(markets)
            );
        hasLocalMarketsRef.current = markets.length > 0;
    }, [markets, address]);

    useEffect(() => {
        if (address)
            localStorage.setItem(
                userKey("cachedMarkets.v1", address),
                JSON.stringify(cachedMarkets)
            );
    }, [cachedMarkets, address]);

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
                        const next = new Set(prev);
                        marketsRef.current.forEach((m) => next.add(m.asset));
                        return Array.from(next);
                    });
                    setMarkets([]);
                }
                return;
            }

            if ("confirmMarket" in payload) {
                const readyMarket = toMarketInfo(payload.confirmMarket);
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
                                  log: [],
                                  trades: [],
                                  strategyName: "",
                                  isPaused: false,
                                  position: null,
                                  engineState: "idle",
                              },
                          ]
                );
                return;
            }

            if ("strategyLog" in payload) {
                const { asset, msg } = payload.strategyLog;
                setMarkets((prev) =>
                    prev.map((m) => {
                        if (m.asset !== asset) return m;
                        const nextLog = [...(m.log ?? []), msg].slice(
                            -MAX_MARKET_LOG_ENTRIES
                        );
                        return { ...m, log: nextLog };
                    })
                );
                return;
            }

            if ("cancelMarket" in payload) {
                const asset = payload.cancelMarket;
                setMarkets((p) => p.filter((m) => m.asset !== asset));
            }

            if ("marketInfoEdit" in payload) {
                const [asset, edit] = payload.marketInfoEdit;
                setMarkets((prev) =>
                    prev.map((m) => {
                        if (m.asset !== asset) return m;
                        if ("lev" in edit) return { ...m, lev: edit.lev };
                        if ("openPosition" in edit)
                            return { ...m, position: edit.openPosition };
                        if ("trade" in edit)
                            return {
                                ...m,
                                trades: [...(m.trades ?? []), edit.trade],
                                pnl: (m.pnl ?? 0) + edit.trade.pnl,
                            };
                        if ("engineState" in edit)
                            return { ...m, engineState: edit.engineState };
                        if ("paused" in edit)
                            return { ...m, isPaused: edit.paused };
                        return m;
                    })
                );
                return;
            }

            if ("backtestProgress" in payload) {
                const { runId, progress } = payload.backtestProgress;
                setBacktestRuns((prev) => {
                    const current = prev[runId];
                    const nextProgress = current
                        ? [...current.progress, progress]
                        : [progress];
                    return {
                        ...prev,
                        [runId]: {
                            runId,
                            progress: nextProgress,
                            latestProgress: progress,
                            result: current?.result ?? null,
                            updatedAt: Date.now(),
                        },
                    };
                });
                return;
            }

            if ("backtestResult" in payload) {
                const { runId, result } = payload.backtestResult;
                setBacktestRuns((prev) => {
                    const current = prev[runId];
                    return {
                        ...prev,
                        [runId]: {
                            runId,
                            progress: current?.progress ?? [],
                            latestProgress: current?.latestProgress ?? null,
                            result,
                            updatedAt: Date.now(),
                        },
                    };
                });
                return;
            }

            if ("needsApiKey" in payload) {
                setNeedsApiKey(payload.needsApiKey as boolean);
                return;
            }

            if ("needsBuilderApproval" in payload) {
                setNeedsBuilderApproval(
                    payload.needsBuilderApproval as boolean
                );
                return;
            }

            if ("updateTotalMargin" in payload) {
                setIsOffline(false);
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

            if ("marketStream" in payload) {
                const stream = payload.marketStream;
                if ("price" in stream) {
                    const { asset, price } = stream.price;
                    setMarkets((prev) =>
                        prev.map((m) =>
                            m.asset === asset
                                ? { ...m, prev: m.price ?? m.prev, price }
                                : m
                        )
                    );
                    return;
                }
                if ("indicators" in stream) {
                    const { asset, data } = stream.indicators;
                    setMarkets((prev) =>
                        prev.map((m) =>
                            m.asset === asset ? { ...m, indicators: data } : m
                        )
                    );
                }
                return;
            }

            if ("userError" in payload) {
                setErrorWithTimeout(payload.userError);
                return;
            }

            if ("loadSession" in payload) {
                const session = payload.loadSession;
                setUniverse(session.universe);
                setNeedsApiKey(!session.agentApproved);
                setNeedsBuilderApproval(!session.builderApproved);
                const deduped = dedupeMarkets(
                    session.markets.map(toMarketInfo)
                );
                hasLocalMarketsRef.current = deduped.length > 0;
                setMarkets(deduped);
            }
        },
        [setErrorWithTimeout]
    );

    const requestSyncMargin = useCallback(async () => {
        await sendCommand({ syncMargin: null });
    }, [sendCommand]);

    const fetchStrategies = useCallback(async () => {
        if (!tokenRef.current) return;
        try {
            const res = await fetch(`${API_URL}/strategies`, {
                headers: { Authorization: `Bearer ${tokenRef.current}` },
            });
            if (res.ok) {
                const data: Strategy[] = await res.json();
                setStrategies(data);
            }
        } catch {
            // silent — strategies are non-critical
        }
    }, []);

    // Fetch strategies on login
    useEffect(() => {
        if (token) fetchStrategies();
    }, [token, fetchStrategies]);

    /** ---------- WS lifecycle ---------- **/
    useEffect(() => {
        if (!token) return;

        let retry = 0;
        let active = true;

        const connect = () => {
            if (!active) return;

            const wsUrl = `${WS_ENDPOINT}?token=${encodeURIComponent(token)}`;
            const ws = new WebSocket(wsUrl);
            wsRef.current = ws;

            ws.addEventListener("open", () => {
                retry = 0;
                sendCommand({ getSession: null }).catch(console.error);
                requestSyncMargin();
                fetchStrategies();
            });

            ws.addEventListener("message", handleMessage);
            ws.addEventListener("close", () => {
                if (!active) return;
                const delay = Math.min(1000 * 2 ** retry, 15000);
                retry++;
                reconnectRef.current = window.setTimeout(connect, delay);
            });

            ws.addEventListener("error", (event) => {
                if (!active) return;
                console.error(event);
                setErrorWithTimeout("WebSocket connection error");
            });
        };

        connect();

        return () => {
            active = false;
            if (reconnectRef.current) {
                clearTimeout(reconnectRef.current);
                reconnectRef.current = null;
            }
            wsRef.current?.close();
            if (errorTimeoutRef.current) {
                clearTimeout(errorTimeoutRef.current);
                errorTimeoutRef.current = null;
            }
        };
    }, [
        token,
        handleMessage,
        sendCommand,
        fetchStrategies,
        requestSyncMargin,
        setErrorWithTimeout,
    ]);

    /** ---------- API ---------- **/
    const cacheMarket = useCallback((asset: string) => {
        setCachedMarkets((prev) =>
            prev.includes(asset) ? prev : [...prev, asset]
        );
    }, []);

    const deleteCachedMarket = useCallback(
        (asset: string) =>
            setCachedMarkets((p) => p.filter((a) => a !== asset)),
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
        },
        [sendCommand]
    );

    const requestCloseAll = useCallback(async () => {
        await sendCommand({ closeAll: null });
        setMarkets([]);
    }, [sendCommand]);

    const requestPauseAll = useCallback(async () => {
        await sendCommand({ pauseAll: null });
    }, [sendCommand]);

    const updateMarketStrategy = useCallback(
        (asset: string, strategyName: string) => {
            setMarkets((prev) =>
                prev.map((m) =>
                    m.asset === asset ? { ...m, strategyName } : m
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
        backtestRuns,
        strategies,
        totalMargin,
        isOffline,
        needsApiKey,
        setNeedsApiKey,
        needsBuilderApproval,
        setNeedsBuilderApproval,
        errorMsg,
        sendCommand,
        dismissError,
        cacheMarket,
        deleteCachedMarket,
        requestRemoveMarket,
        requestToggleMarket,
        requestCloseAll,
        requestPauseAll,
        requestSyncMargin,
        fetchStrategies,
        updateMarketStrategy,
    };

    return (
        <WebSocketContext.Provider value={value}>
            {children}
        </WebSocketContext.Provider>
    );
};
