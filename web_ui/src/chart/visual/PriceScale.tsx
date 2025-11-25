import React, { useRef, useEffect } from "react";
import { useChartContext } from "../ChartContext";
import {
    attachVerticalDrag,
    zoomPriceRange,
    priceToY,
    yToPrice,
    handleWheelZoom,
} from "../utils";

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
        selectingInterval
    } = useChartContext();

    const svgRef = useRef<SVGSVGElement>(null);
    useEffect(() => {
        const node = svgRef.current;
        if (!node) return;

        const blockScroll = (e: WheelEvent) => {
            e.preventDefault();
        };

        node.addEventListener("wheel", blockScroll, { passive: false });

        return () => node.removeEventListener("wheel", blockScroll);
    }, []);

    const levels = 14;
    const step = (maxPrice - minPrice) / (levels - 1);

    const prices = Array.from({ length: levels }, (_, i) => {
        const price = minPrice + i * step;
        const y = priceToY(price, minPrice, maxPrice, height);
        return { price, y };
    });

    const crosshairPrice =
        crosshairY !== null
            ? yToPrice(crosshairY, minPrice, maxPrice, height)
            : null;

    const onWheel = (e: React.WheelEvent) => {
        e.stopPropagation();

        const { min, max } = handleWheelZoom(minPrice, maxPrice, e.deltaY);

        setManualPriceRange(true);
        setPriceRange(min, max);
    };

    return (
        <svg
            width={700}
            height={height}
            style={{ overflow: "visible" }}
            ref={svgRef}
            onWheel={onWheel}
            onMouseDown={(e) => {
                e.preventDefault();

                const startMin = minPrice;
                const startMax = maxPrice;
                const startY = e.clientY;

                const handleMove = (ev: MouseEvent) => {
                    const dy = ev.clientY - startY; // TOTAL drag distance
                    const { min, max } = zoomPriceRange(startMin, startMax, dy);
                    setManualPriceRange(true);
                    setPriceRange(min, max);
                };

                const handleUp = () => {
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
                        {p.price.toFixed(2)}
                    </text>
                </g>
            ))}

            {/* --- Crosshair Price Label --- */}
            {crosshairPrice !== null && mouseOnChart && !selectingInterval &&(
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
                        {crosshairPrice.toFixed(2)}
                    </text>
                </>
            )}
        </svg>
    );
};

export default PriceScale;
