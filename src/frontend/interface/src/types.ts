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
    pnl: number,
    is_paused: boolean,
    indicators: IndicatorKind[],

}


