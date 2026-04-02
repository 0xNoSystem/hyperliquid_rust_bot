import type { ExchangeId, MarketType } from "./types";

export const EXCHANGE_OPTIONS: {
    value: ExchangeId;
    label: string;
    markets: MarketType[];
}[] = [
    { value: "binance", label: "Binance", markets: ["spot", "futures"] },
    { value: "bybit", label: "Bybit", markets: ["spot", "futures"] },
    { value: "htx", label: "HTX", markets: ["spot", "futures"] },
];

export const MARKET_OPTIONS: { value: MarketType; label: string }[] = [
    { value: "spot", label: "Spot" },
    { value: "futures", label: "Futures" },
];

const DEFAULT_MARKETS = MARKET_OPTIONS.map((option) => option.value);

export const getMarketsForExchange = (exchange: ExchangeId): MarketType[] => {
    const match = EXCHANGE_OPTIONS.find((item) => item.value === exchange);
    return match ? match.markets : DEFAULT_MARKETS;
};
