import type { TimeFrame } from "../types";

export type { TimeFrame };

export type MarketType = "spot" | "futures";

export type ExchangeId =
    | "binance"
    | "bybit"
    | "okx"
    | "coinbase"
    | "kraken"
    | "kucoin"
    | "bitget"
    | "gateio"
    | "htx"
    | "mexc";

export type DataSource = {
    exchange: ExchangeId;
    market: MarketType;
};

export const DEFAULT_DATA_SOURCE: DataSource = {
    exchange: "binance",
    market: "futures",
};

export const DEFAULT_QUOTE_ASSET = "USDT";

export interface CandleData {
    open: number;
    high: number;
    low: number;
    close: number;
    start: number;
    end: number;
    volume: number;
    trades: number;
    asset: string;
    interval: string;
}
