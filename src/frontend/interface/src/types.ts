export type IndicatorKind =
  | { rsi: number }
  | { smaOnRsi: { periods: number; smoothingLength: number } }
  | { stochRsi: { periods: number; kSmoothing?: number | null; dSmoothing?: number | null } }
  | { adx: { periods: number; diLength: number } }
  | { atr: number }
  | { ema: number }
  | { emaCross: { short: number; long: number } }
  | { sma: number };

export interface MarketInfo{
    asset: string, 
    lev: number,
    price: number,
    margin: number,
    params: TradeParams,
    pnl: number,
    is_paused: boolean,
    indicators: IndexId[],
}






export type TimeFrame =
  | "min1"
  | "min3"
  | "min5"
  | "min15"
  | "min30"
  | "hour1"
  | "hour2"
  | "hour4"
  | "hour12"
  | "day1"
  | "day3"
  | "week"
  | "month";

export const TIMEFRAME_CAMELCASE: Record<string, TimeFrame> = {
  "1m": "min1",
  "3m": "min3",
  "5m": "min5",
  "15m": "min15",
  "30m": "min30",
  "1h": "hour1",
  "2h": "hour2",
  "4h": "hour4",
  "12h": "hour12",
  "1d": "day1",
  "3d": "day3",
  "w": "week",
  "m": "month",
};

export function into(tf: string): TimeFrame {
  return TIMEFRAME_CAMELCASE[tf];
}


export type Risk = "Low" | "Normal" | "High";
export type Style = "Scalp" | "Swing";
export type Stance = "Bull" | "Bear" | "Neutral";

export interface CustomStrategy {
  risk: Risk;
  style: Style;
  stance: Stance;
  followTrend: boolean;
}

export type Strategy = { custom: CustomStrategy };

export interface TradeParams {
  timeFrame: TimeFrame;  
  lev: number;
  strategy: Strategy;
  tradeTime: number;
}

export type MarginAllocation =
  | {alloc: number }
  | {amount: number };


export type IndexId = [IndicatorKind, TimeFrame];


export interface AddMarketInfo {
  asset: string;
  marginAlloc: MarginAllocation;
  tradeParams: TradeParams;
  config?: IndexId[];
};

export type Message = 
    | { confirmMarket: MarketInfo }
    | { updatePrice: assetPrice }
    | { newTradeInfo: TradeInfo }
    | { updateTotalMargin: number}
    | { updateMarketMargin: assetMargin }
    | { updateIndicatorValues: {asset: string, data: indicatorData[] }}
    | { marketInfoEdit: [string, editMarketInfo]}
    | { userError: string };


export type assetPrice = [string, number];
export type assetMargin = [string, number];


export interface indicatorData {
    id: IndexId,
    value?: number 
};

export type editMarketInfo = 
    | {lev: number}
    | {strategy: Strategy}
    | {margin: number};


export interface TradeInfo{
    open: number,
    close: number,
    pnl: number,
    fee: number,
    is_long: number,
    duration?: number,
    oid: [number, number]
};


export const indicatorLabels: Record<string, string> = {
  rsi: 'RSI',
  smaOnRsi: 'SMA on RSI',
  stochRsi: 'Stoch RSI',
  adx: 'ADX',
  atr: 'ATR',
  ema: 'EMA',
  emaCross: 'EMA Cross',
  sma: 'SMA',
};

export const indicatorColors: Record<string, string> = {
  rsi: 'bg-green-800 text-green-200',
  smaOnRsi: 'bg-indigo-800 text-indigo-200',
  stochRsi: 'bg-purple-800 text-purple-200',
  adx: 'bg-yellow-800 text-yellow-200',
  atr: 'bg-red-800 text-red-200',
  ema: 'bg-blue-800 text-blue-200',
  emaCross: 'bg-pink-800 text-pink-200',
  sma: 'bg-gray-800 text-gray-200',
};









