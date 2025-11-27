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

        console.log(fetchStart);

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

export function yToPrice(
    y: number,
    minPrice: number,
    maxPrice: number,
    height: number
): number {
    const normalized = 1 - y / height;
    return minPrice + normalized * (maxPrice - minPrice);
}

export function timeToX(
    time: number,
    startTime: number,
    endTime: number,
    width: number
): number {
    if (endTime === startTime) return width / 2; // avoid division by zero

    const normalized = (time - startTime) / (endTime - startTime);
    return normalized * width;
}

export function xToTime(
    x: number,
    startTime: number,
    endTime: number,
    width: number
): number {
    const normalized = x / width;
    return startTime + normalized * (endTime - startTime);
}

export function formatUTC(ms: number): string {
    const d = new Date(ms);
    const day = d.getUTCDate();
    const month = d.toLocaleString("en-US", {
        month: "short",
        timeZone: "UTC",
    }); // Sep
    const year = d.getUTCFullYear();

    const hh = String(d.getUTCHours()).padStart(2, "0");
    const mm = String(d.getUTCMinutes()).padStart(2, "0");

    return `${day} ${month} '${year - 2000} ${hh}:${mm}`;
}

export function zoomPriceRange(
    initialMin: number,
    initialMax: number,
    totalDy: number
) {
    const initialRange = initialMax - initialMin;
    const center = initialMin + initialRange / 2;

    // dy>0 zoom out, dy<0 zoom in
    const speed = 0.002;
    const factor = Math.max(0.1, 1 + totalDy * speed);

    const newRange = initialRange * factor;

    return {
        min: center - newRange / 2,
        max: center + newRange / 2,
    };
}

export function attachVerticalDrag(
    onMove: (dy: number) => void,
    onEnd?: () => void
) {
    const handleMove = (e: MouseEvent) => {
        onMove(e.movementY); // vertical delta
    };

    const handleUp = () => {
        window.removeEventListener("mousemove", handleMove);
        window.removeEventListener("mouseup", handleUp);
        onEnd?.();
    };

    window.addEventListener("mousemove", handleMove);
    window.addEventListener("mouseup", handleUp);
}

export function handleWheelZoom(
    minPrice: number,
    maxPrice: number,
    deltaY: number
) {
    const range = maxPrice - minPrice;
    const center = (minPrice + maxPrice) / 2;

    const speed = 0.001;
    const factor = 1 + deltaY * speed;

    const newRange = Math.max(0.000001, range * factor);

    return {
        min: center - newRange / 2,
        max: center + newRange / 2,
    };
}

// Pan price range vertically by translating the visible window
export function computePricePan(
    initialMin: number,
    initialMax: number,
    totalDy: number,
    height: number
) {
    const range = initialMax - initialMin;
    if (height <= 0 || range === 0) {
        return { min: initialMin, max: initialMax };
    }

    const pricePerPixel = range / height;
    const shift = totalDy * pricePerPixel;

    return {
        min: initialMin + shift,
        max: initialMax + shift,
    };
}

// Zoom time range with mouse wheel
export function computeTimeWheelZoom(
    startTime: number,
    endTime: number,
    deltaY: number
) {
    const range = endTime - startTime;
    const center = (startTime + endTime) / 2;

    const speed = 0.0015;
    const factor = 1 + deltaY * speed;

    const newRange = Math.max(1, range * factor); // prevents collapse

    return {
        start: center - newRange / 2,
        end: center + newRange / 2,
    };
}

// Pan time range horizontally via drag
export function computeTimePan(
    initialStart: number,
    initialEnd: number,
    totalDx: number,
    width: number
) {
    // convert pixel movement to time delta
    const range = initialEnd - initialStart;
    const timePerPixel = range / width;
    const shift = totalDx * timePerPixel;

    return {
        start: initialStart - shift,
        end: initialEnd - shift,
    };
}
