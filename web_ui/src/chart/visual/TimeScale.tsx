import React from "react";
import { useChartContext } from "../ChartContext";
import { timeToX, xToTime, formatUTC } from "../utils";

const TimeScale: React.FC = () => {
    const { width, height, startTime, endTime, crosshairX, mouseOnChart } =
        useChartContext();

    const ticks = 12;
    const step = (endTime - startTime) / (ticks - 1);

    const times = Array.from({ length: ticks }, (_, i) => {
        const t = startTime + i * step;
        const x = timeToX(t, startTime, endTime, width);
        return { t, x };
    });

    const crosshairTime =
        crosshairX !== null
            ? xToTime(crosshairX, startTime, endTime, width)
            : null;

    return (
        <svg width={width} height={25} style={{ overflow: "visible" }}>
            {/* tick labels */}
            {times.slice(0, -1).map((p, idx) => (
                <g key={idx}>
                    <line
                        x1={p.x}
                        y1={0}
                        x2={p.x}
                        y2={-height - 10}
                        stroke="#444"
                        strokeOpacity={0.4}
                        strokeWidth={0.8}
                    />
                    <text
                        x={p.x}
                        y={20}
                        textAnchor="middle"
                        fill="#aaa"
                        fontSize={11}
                    >
                        {formatUTC(p.t)}
                    </text>
                </g>
            ))}

            {/* crosshair label */}
            {crosshairTime !== null && mouseOnChart && (
                <>
                    <rect
                        x={crosshairX - 60}
                        y={0}
                        width={120}
                        height={18}
                        fill="#2a2a2a"
                        stroke="#ffffff44"
                        strokeWidth={1}
                        rx={4}
                    />

                    <text
                        x={crosshairX}
                        y={13}
                        textAnchor="middle"
                        fill="white"
                        fontSize={12}
                        fontWeight="bold"
                    >
                        {formatUTC(crosshairTime)}
                    </text>
                </>
            )}
        </svg>
    );
};

export default TimeScale;
