import React, { useRef, useEffect, useMemo } from "react";
import { useChartContext } from "../ChartContext";
import {
    zoomPriceRange,
    priceToY,
    yToPrice,
    handleWheelZoom,
    computePricePan,
} from "../utils";

const MAX_DECIMALS = 10;

const clampDecimals = (value: number) =>
    Math.min(MAX_DECIMALS, Math.max(0, Math.round(value)));

const formatFixedPrice = (value: number, decimals: number) => {
    if (!Number.isFinite(value)) return "—";
    const safeDecimals = clampDecimals(decimals);
    return value.toLocaleString("en-US", {
        minimumFractionDigits: safeDecimals,
        maximumFractionDigits: safeDecimals,
    });
};

const inferAssetDecimals = (price: number) => {
    const abs = Math.abs(price);
    if (!Number.isFinite(abs) || abs === 0) return 2;
    if (abs >= 1) return 2;
    const magnitude = Math.max(abs, 1e-12);
    const leadingZeros = Math.max(0, -Math.floor(Math.log10(magnitude)));
    return Math.max(6, leadingZeros + 2);
};

const niceStep = (rawStep: number) => {
    if (!Number.isFinite(rawStep) || rawStep <= 0) return 0;
    const exponent = Math.floor(Math.log10(rawStep));
    const base = 10 ** exponent;
    const fraction = rawStep / base;
    if (fraction <= 1) return 1 * base;
    if (fraction <= 2) return 2 * base;
    if (fraction <= 2.5) return 2.5 * base;
    if (fraction <= 5) return 5 * base;
    return 10 * base;
};

const PriceScale: React.FC = () => {
    const {
        height,
        minPrice,
        maxPrice,
        candles,
        setPriceRange,
        setManualPriceRange,
        width,
        crosshairY,
        mouseOnChart,
        selectingInterval,
    } = useChartContext();

    const svgRef = useRef<SVGSVGElement>(null);
    const dragModeRef = useRef<"zoom" | "pan">("zoom");
    const assetDecimalsRef = useRef<{ asset: string; decimals: number } | null>(
        null
    );
    const touchState = useRef<{
        mode: "zoom" | "pinch";
        startY: number;
        startDistance?: number;
        initialMin: number;
        initialMax: number;
    } | null>(null);
    useEffect(() => {
        const node = svgRef.current;
        if (!node) return;

        const blockScroll = (e: WheelEvent) => {
            e.preventDefault();
        };

        node.addEventListener("wheel", blockScroll, { passive: false });

        return () => node.removeEventListener("wheel", blockScroll);
    }, []);

    useEffect(() => {
        const node = svgRef.current;
        if (!node) return;

        const blockTouch = (e: TouchEvent) => e.preventDefault();
        node.addEventListener("touchstart", blockTouch, { passive: false });
        node.addEventListener("touchmove", blockTouch, { passive: false });

        return () => {
            node.removeEventListener("touchstart", blockTouch);
            node.removeEventListener("touchmove", blockTouch);
        };
    }, []);

    const range = maxPrice - minPrice;
    const fontSize = Math.max(10, Math.min(16, height * 0.06));
    const plotPadding = Math.max(6, Math.round(fontSize / 2));
    const targetPx = 42;
    const referencePrice = useMemo(() => {
        if (candles.length > 0) {
            const last = candles[candles.length - 1];
            if (Number.isFinite(last.close)) return last.close;
        }
        const mid = (minPrice + maxPrice) / 2;
        if (Number.isFinite(mid)) return mid;
        if (Number.isFinite(minPrice)) return minPrice;
        if (Number.isFinite(maxPrice)) return maxPrice;
        return 0;
    }, [candles, minPrice, maxPrice]);

    const assetKey = candles[0]?.asset ?? "unknown";
    const candidateDecimals = clampDecimals(inferAssetDecimals(referencePrice));
    let assetDecimals = candidateDecimals;
    const stored = assetDecimalsRef.current;
    if (stored && stored.asset === assetKey) {
        assetDecimals = stored.decimals;
    } else if (candles.length > 0 && assetKey !== "unknown") {
        assetDecimalsRef.current = {
            asset: assetKey,
            decimals: candidateDecimals,
        };
        assetDecimals = candidateDecimals;
    }

    const rawStep =
        height > 0 ? range / Math.max(2, Math.floor(height / targetPx)) : 0;
    const minStep = assetDecimals > 0 ? 10 ** -assetDecimals : 1;
    const stepBase = rawStep > 0 ? Math.max(rawStep, minStep) : rawStep;
    const step = niceStep(stepBase);

    const formatAxisPrice = (value: number) => {
        if (!Number.isFinite(value)) return "—";
        if (step <= 0) return formatFixedPrice(value, assetDecimals);
        const rounded = Math.round(value / step) * step;
        const safeValue = Math.abs(rounded) < step / 2 ? 0 : rounded;
        return formatFixedPrice(safeValue, assetDecimals);
    };
    const formatCrosshairPrice = (value: number) => {
        if (!Number.isFinite(value)) return "—";
        return formatFixedPrice(value, assetDecimals);
    };

    const prices: { price: number; y: number; major: boolean }[] = [];
    if (step > 0 && range > 0 && height > 0) {
        const minorStep = step / 2;
        const pxPerUnit = height / range;
        const minorSpacing = minorStep * pxPerUnit;
        const showMinor = minorSpacing >= 14;
        const loopStep = showMinor ? minorStep : step;
        const first = Math.floor(minPrice / loopStep) * loopStep;
        const last = Math.ceil(maxPrice / loopStep) * loopStep;
        const epsilon = step * 1e-6;
        for (
            let price = first;
            price <= last + loopStep * 0.5;
            price += loopStep
        ) {
            const y = priceToY(price, minPrice, maxPrice, height);
            if (y < plotPadding || y > height - plotPadding) continue;
            const major =
                Math.abs(price / step - Math.round(price / step)) <= epsilon;
            if (!showMinor && !major) continue;
            prices.push({ price, y, major });
            if (prices.length > 300) break;
        }
    }

    const crosshairPrice =
        crosshairY !== null
            ? yToPrice(crosshairY, minPrice, maxPrice, height)
            : null;
    const crosshairYValue = crosshairY ?? 0;

    const onTouchStart = (e: React.TouchEvent) => {
        if (e.touches.length === 1) {
            touchState.current = {
                mode: "zoom",
                startY: e.touches[0].clientY,
                initialMin: minPrice,
                initialMax: maxPrice,
            };
        } else if (e.touches.length >= 2) {
            const distance = Math.hypot(
                e.touches[1].clientX - e.touches[0].clientX,
                e.touches[1].clientY - e.touches[0].clientY
            );
            touchState.current = {
                mode: "pinch",
                startY: 0,
                startDistance: Math.max(1, distance),
                initialMin: minPrice,
                initialMax: maxPrice,
            };
        }
    };

    const onTouchMove = (e: React.TouchEvent) => {
        if (!touchState.current) return;

        const state = touchState.current;

        if (state.mode === "zoom" && e.touches.length === 1) {
            const dy = e.touches[0].clientY - state.startY;
            const { min, max } = zoomPriceRange(
                state.initialMin,
                state.initialMax,
                dy
            );
            setManualPriceRange(true);
            setPriceRange(min, max);
            return;
        }

        if (state.mode === "pinch" && e.touches.length >= 2) {
            const distance = Math.hypot(
                e.touches[1].clientX - e.touches[0].clientX,
                e.touches[1].clientY - e.touches[0].clientY
            );
            if (!state.startDistance) return;

            const initialRange = state.initialMax - state.initialMin;
            if (initialRange <= 0) return;

            const scale = state.startDistance / Math.max(1, distance);
            const newRange = Math.max(0.000001, initialRange * scale);
            const center = (state.initialMin + state.initialMax) / 2;
            const min = center - newRange / 2;
            const max = center + newRange / 2;

            setManualPriceRange(true);
            setPriceRange(min, max);
        }
    };

    const onTouchEnd = (e: React.TouchEvent) => {
        if (e.touches.length === 1) {
            touchState.current = {
                mode: "zoom",
                startY: e.touches[0].clientY,
                initialMin: minPrice,
                initialMax: maxPrice,
            };
            return;
        }

        if (e.touches.length === 0) {
            touchState.current = null;
        }
    };

    const onWheel = (e: React.WheelEvent) => {
        e.stopPropagation();

        if (e.shiftKey) {
            const { min, max } = computePricePan(
                minPrice,
                maxPrice,
                e.deltaY,
                height
            );
            setManualPriceRange(true);
            setPriceRange(min, max);
            return;
        }

        const { min, max } = handleWheelZoom(minPrice, maxPrice, e.deltaY);

        setManualPriceRange(true);
        setPriceRange(min, max);
    };

    const labelWidth = Math.max(80, Math.min(180, width * 0.085));
    const labelX = labelWidth / 2;
    const crosshairWidth = Math.max(80, Math.min(140, labelWidth - 8));
    const crosshairX = (labelWidth - crosshairWidth) / 2;

    return (
        <svg
            width={labelWidth}
            height={height}
            style={{
                overflowX: "visible",
                overflowY: "visible",
                touchAction: "none",
                overscrollBehavior: "contain",
            }}
            ref={svgRef}
            onWheel={onWheel}
            onTouchStart={onTouchStart}
            onTouchMove={onTouchMove}
            onTouchEnd={onTouchEnd}
            onTouchCancel={onTouchEnd}
            onMouseDown={(e) => {
                e.preventDefault();

                dragModeRef.current =
                    e.shiftKey || e.button === 1 ? "pan" : "zoom";

                const startMin = minPrice;
                const startMax = maxPrice;
                const startY = e.clientY;

                const handleMove = (ev: MouseEvent) => {
                    const dy = ev.clientY - startY; // TOTAL drag distance
                    const { min, max } =
                        dragModeRef.current === "pan"
                            ? computePricePan(startMin, startMax, dy, height)
                            : zoomPriceRange(startMin, startMax, dy);
                    setManualPriceRange(true);
                    setPriceRange(min, max);
                };

                const handleUp = () => {
                    dragModeRef.current = "zoom";
                    window.removeEventListener("mousemove", handleMove);
                    window.removeEventListener("mouseup", handleUp);
                };

                window.addEventListener("mousemove", handleMove);
                window.addEventListener("mouseup", handleUp);
            }}
        >
            {/* Regular scale labels */}
            {prices.map((p, idx) => (
                <g key={idx}>
                    <line
                        x1={0} // a bit to the right of the text
                        y1={p.y}
                        x2={-width} // full width line
                        y2={p.y}
                        stroke="#444"
                        strokeOpacity={p.major ? 0.4 : 0.22}
                        strokeWidth={p.major ? 0.8 : 0.6}
                    />
                    {p.major && (
                        <text
                            x={labelX}
                            y={p.y}
                            textAnchor="middle"
                            alignmentBaseline="middle"
                            fill="#aaa"
                            fontSize={fontSize - 2}
                        >
                            {formatAxisPrice(p.price)}
                        </text>
                    )}
                </g>
            ))}

            {/* --- Crosshair Price Label --- */}
            {crosshairY !== null &&
                crosshairPrice !== null &&
                mouseOnChart &&
                !selectingInterval && (
                    <>
                        {/* Background box (TV style) */}
                        <rect
                            x={crosshairX}
                            y={crosshairYValue - 9}
                            width={crosshairWidth}
                            height={18}
                            fill="#2a2a2a"
                            stroke="#ffffff44"
                            strokeWidth={1}
                            rx={4}
                        />

                        {/* Price text */}
                        <text
                            x={labelX}
                            y={crosshairYValue}
                            textAnchor="middle"
                            alignmentBaseline="middle"
                            fill="white"
                            fontSize={fontSize + 1}
                            fontWeight="bold"
                        >
                            {formatCrosshairPrice(crosshairPrice)}
                        </text>
                    </>
                )}
        </svg>
    );
};

export default PriceScale;
