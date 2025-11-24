function scaleY(price, min, max, height) {
    return height - ((price - min) / (max - min)) * height;
}

function scaleX(i, count, width) {
    return (i / count) * width;
}

export async function fetchCandles(
    asset: string,
    startTime: number,
    endTime: number,
    interval: string
): Promise<CandleData[]> {
    const symbol = asset.toUpperCase() + "USDT";
    const all: CandleData[] = [];

    let fetchStart = startTime;

    while (fetchStart < endTime) {
        const url =
            `https://api.binance.com/api/v3/klines?symbol=${symbol}` +
            `&interval=${interval}&startTime=${fetchStart}&endTime=${endTime}&limit=1000`;

        const res = await fetch(url);
        if (!res.ok) throw new Error(`Binance error ${res.status}`);

        const data: any[] = await res.json();
        if (data.length === 0) break;

        // Map into CandleData
        const mapped = data.map((k) => ({
            start: k[0],
            open: parseFloat(k[1]),
            high: parseFloat(k[2]),
            low: parseFloat(k[3]),
            close: parseFloat(k[4]),
            volume: parseFloat(k[5]),
            end: k[6],
            trades: k[8],
            asset,
            interval,
        }));

        all.push(...mapped);

        // Move to next candle
        fetchStart = data[data.length - 1][0] + 1;

        // Safety break (avoid infinite loops)
        if (data.length < 1000) break;
    }

    return all;
}



export interface HyperliquidCandle {
    T: number;
    c: string;
    h: string;
    i: string;
    l: string;
    n: number;
    o: string;
    s: string;
    t: number;
    v: string;
}

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

function parseCandle(raw: HyperliquidCandle): CandleData {
    return {
        open: parseFloat(raw.o),
        high: parseFloat(raw.h),
        low: parseFloat(raw.l),
        close: parseFloat(raw.c),
        start: raw.t,
        end: raw.T,
        volume: parseFloat(raw.v),
        trades: raw.n,
        asset: raw.s,
        interval: raw.i,
    };
}

export function priceToY(
    price: number,
    minPrice: number,
    maxPrice: number,
    height: number
): number {
    if (maxPrice === minPrice) return height / 2; // avoid BOOM

    const normalized = (price - minPrice) / (maxPrice - minPrice);
    return height - normalized * height;
}
