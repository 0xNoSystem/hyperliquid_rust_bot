import React, { useRef, useEffect } from "react";
import { useChartContext } from "../ChartContext";
import {
    zoomPriceRange,
    priceToY,
    yToPrice,
    handleWheelZoom,
    computePricePan,
} from "../utils";

const formatPrice = (n: number) => {
    if (n > 1 && n < 2) return n.toFixed(4);
    if (n < 1) return n.toFixed(6);
    if (n > 10000) return Number(n.toFixed(0)).toLocaleString("en-US");
    return Number(n.toFixed(2)).toLocaleString("en-US");
};


const PriceScale: React.FC = () => {
    const {
        height,
        minPrice,
        maxPrice,
        setPriceRange,
        setManualPriceRange,
        width,
        crosshairY,
        mouseOnChart,
        selectingInterval,
    } = useChartContext();

    const svgRef = useRef<SVGSVGElement>(null);
    const dragModeRef = useRef<"zoom" | "pan">("zoom");
    useEffect(() => {
        const node = svgRef.current;
        if (!node) return;

        const blockScroll = (e: WheelEvent) => {
            e.preventDefault();
        };

        node.addEventListener("wheel", blockScroll, { passive: false });

        return () => node.removeEventListener("wheel", blockScroll);
    }, []);

    const levels = 12;
    const step = (maxPrice - minPrice) / (levels - 1);

    const prices = Array.from({ length: levels }, (_, i) => {
        const price = minPrice + i * step * 0.95;
        const y = priceToY(price, minPrice, maxPrice, height) * 0.95;
        return { price, y };
    });

    const crosshairPrice =
        crosshairY !== null
            ? yToPrice(crosshairY, minPrice, maxPrice, height)
            : null;

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

    return (
        <svg
            width={100}
            height={height}
            style={{ overflow: "visible" }}
            ref={svgRef}
            onWheel={onWheel}
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
                <g>
                    <line
                        x1={0} // a bit to the right of the text
                        y1={p.y}
                        x2={-width} // full width line
                        y2={p.y}
                        stroke="#444"
                        strokeOpacity={0.4}
                        strokeWidth={0.8}
                    />

                    <text
                        key={idx}
                        x={65}
                        y={p.y}
                        textAnchor="end"
                        alignmentBaseline="middle"
                        fill="#aaa"
                        fontSize={14}
                    >
                        {formatPrice(p.price)}
                    </text>
                </g>
            ))}

            {/* --- Crosshair Price Label --- */}
            {crosshairPrice !== null && mouseOnChart && !selectingInterval && (
                <>
                    {/* Background box (TV style) */}
                    <rect
                        x={5}
                        y={crosshairY - 9}
                        width={60}
                        height={18}
                        fill="#2a2a2a"
                        stroke="#ffffff44"
                        strokeWidth={1}
                        rx={4}
                    />

                    {/* Price text */}
                    <text
                        x={65}
                        y={crosshairY}
                        textAnchor="end"
                        alignmentBaseline="middle"
                        fill="white"
                        fontSize={12}
                        fontWeight="bold"
                    >
                        {formatPrice(crosshairPrice)}
                    </text>
                </>
            )}
        </svg>
    );
};

export default PriceScale;
