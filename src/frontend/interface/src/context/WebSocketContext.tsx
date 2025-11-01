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
import { market_add_info } from "../types";

const CACHED_MARKETS_KEY = "cachedMarkets.v1";

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
  undefined,
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
  markets.forEach((m) => {
    map.set(m.asset, m);
  });
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

  const sendCommand = useCallback(async (body: unknown) => {
    const res = await fetch("http://localhost:8090/command", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!res.ok) {
      throw new Error(`Command failed: ${res.status} ${res.statusText}`);
    }
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

  // 2) Inside WebSocketProvider, after state declarations
useEffect(() => {
  try {
    const raw = localStorage.getItem(CACHED_MARKETS_KEY);
    if (raw) setCachedMarkets(JSON.parse(raw));
  } catch {}
}, []);

useEffect(() => {
  try {
    localStorage.setItem(CACHED_MARKETS_KEY, JSON.stringify(cachedMarkets));
  } catch {}
}, [cachedMarkets]);


useEffect(() => {
  const onStorage = (e: StorageEvent) => {
    if (e.key === CACHED_MARKETS_KEY && e.newValue) {
      try { setCachedMarkets(JSON.parse(e.newValue)); } catch {}
    }
  };
  window.addEventListener("storage", onStorage);
  return () => window.removeEventListener("storage", onStorage);
}, []);


  const handleMessage = useCallback((event: MessageEvent) => {
    const payload = JSON.parse(event.data) as Message;

    if ("confirmMarket" in payload) {
      const asset = payload.confirmMarket.asset;
      setMarkets((prev) => {
        const hasAsset = prev.some((m) => m.asset === asset);
        const updated = hasAsset
          ? prev.map((m) =>
              m.asset === asset
                ? { ...payload.confirmMarket, state: "Ready" }
                : m,
            )
          : [...prev, { ...payload.confirmMarket, state: "Ready" }];
        return dedupeMarkets(updated);
      });
      return;
    }

    if ("preconfirmMarket" in payload) {
      const asset = payload.preconfirmMarket;
      setMarkets((prev) => {
        const exists = prev.some((m) => m.asset === asset);
        if (exists) return prev;
        return [
          ...prev,
          {
            asset,
            state: "Loading",
            price: null,
            lev: null,
            margin: null,
            pnl: null,
            indicators: [],
            trades: [],
            params: DEFAULT_PLACEHOLDER_PARAMS,
            isPaused: false,
          },
        ];
      });
      return;
    }

    if ("updatePrice" in payload) {
      const [asset, price] = payload.updatePrice as assetPrice;
      setMarkets((prev) => {
        if (!prev.some((m) => m.asset === asset)) return prev;
        return prev.map((m) => (m.asset === asset ? { ...m, price } : m));
      });
      return;
    }

    if ("newTradeInfo" in payload) {
      const { asset, info } = payload.newTradeInfo as MarketTradeInfo;
      setMarkets((prev) => {
        if (!prev.some((m) => m.asset === asset)) return prev;
        return prev.map((m) => {
          if (m.asset !== asset) return m;
          const trades = Array.isArray(m.trades) ? [...m.trades, info] : [info];
          const nextPnl = (m.pnl ?? 0) + info.pnl;
          return { ...m, trades, pnl: nextPnl };
        });
      });
      return;
    }

    if ("marketInfoEdit" in payload){
        const [asset, edit] = payload.marketInfoEdit as [string, editMarketInfo];
        if (edit.lev){
            setMarkets((prev) =>{
                return prev.map((m) => (m.asset === asset ? {...m, lev: edit.lev} : m));
            });
        }
    }
        

    if ("updateTotalMargin" in payload) {
      setTotalMargin(payload.updateTotalMargin);
      return;
    }

    if ("updateMarketMargin" in payload) {
      const [asset, margin] = payload.updateMarketMargin as assetMargin;
      setMarkets((prev) => {
        if (!prev.some((m) => m.asset === asset)) return prev;
        return prev.map((m) => (m.asset === asset ? { ...m, margin } : m));
      });
      return;
    }

    if ("updateIndicatorValues" in payload) {
      const { asset, data } = payload.updateIndicatorValues;
      setMarkets((prev) => {
        if (!prev.some((m) => m.asset === asset)) return prev;
        return prev.map((m) =>
          m.asset === asset ? { ...m, indicators: data } : m,
        );
      });
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
      setMarkets(dedupeMarkets(sessionMarkets));
      setUniverse(meta);
      return;
    }
  }, [setErrorWithTimeout]);

  useEffect(() => {
    if (wsRef.current) return;

    const connect = () => {
      const ws = new WebSocket("ws://localhost:8090/ws");
      wsRef.current = ws;

      ws.onopen = () => {
        sendCommand({ getSession: null }).catch((err) =>
          console.error("Failed to load session", err),
        );
      };

      ws.onmessage = handleMessage;
      ws.onerror = (err) => console.error("WebSocket error", err);
      ws.onclose = () => {
        if (reconnectRef.current) clearTimeout(reconnectRef.current);
        reconnectRef.current = window.setTimeout(connect, 1000);
      };
    };

    connect();

    return () => {
      if (reconnectRef.current) clearTimeout(reconnectRef.current);
      if (wsRef.current) {
        wsRef.current.close();
        wsRef.current = null;
      }
      if (errorTimeoutRef.current) {
        clearTimeout(errorTimeoutRef.current);
      }
    };
  }, [handleMessage, sendCommand]);

  const cacheMarket = useCallback((market: MarketInfo) => {
    setCachedMarkets((prev) => {
      const cached = market_add_info(market);
      return prev.some((m) => m.asset === cached.asset)
        ? prev
        : [...prev, cached];
    });
  }, []);

  const deleteCachedMarket = useCallback((asset: string) => {
    setCachedMarkets((prev) => prev.filter((m) => m.asset !== asset));
  }, []);

  const requestRemoveMarket = useCallback(
    async (asset: string) => {
      await sendCommand({ removeMarket: asset.toUpperCase() });
      setMarkets((prev) => prev.filter((m) => m.asset !== asset));
    },
    [sendCommand],
  );

  const requestToggleMarket = useCallback(
    async (asset: string) => {
      await sendCommand({ toggleMarket: asset.toUpperCase() });
      setMarkets((prev) =>
        prev.map((m) =>
          m.asset === asset ? { ...m, isPaused: !m.isPaused } : m,
        ),
      );
    },
    [sendCommand],
  );

  const requestCloseAll = useCallback(async () => {
    await sendCommand({ closeAll: null });
    setMarkets([]);
  }, [sendCommand]);

  const requestPauseAll = useCallback(async () => {
    await sendCommand({ pauseAll: null });
    setMarkets((prev) => prev.map((m) => ({ ...m, isPaused: true })));
  }, [sendCommand]);

  const dismissError = useCallback(() => {
    setErrorWithTimeout(null);
  }, [setErrorWithTimeout]);

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
  if (!ctx) {
    throw new Error("useWebSocketContext must be used within WebSocketProvider");
  }
  return ctx;
};
