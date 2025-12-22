import { useChartContext } from "../ChartContextStore";

const CrossHair = () => {
    const { crosshairX, crosshairY, height, width } = useChartContext();
    if (crosshairX === null || crosshairY === null) return null;

    // Keep the crosshair inside the drawable area and snap to whole pixels for crisper lines.
    const clampedX = Math.min(Math.max(crosshairX, 0), width);
    const clampedY = Math.min(Math.max(crosshairY, 0), height);
    const crispX = Math.round(clampedX) + 0.5;
    const crispY = Math.round(clampedY) + 0.5;

    return (
        <g pointerEvents="none" className="z-10">
            {/* Vertical line */}
            <line
                x1={crispX}
                y1={0}
                x2={crispX}
                y2={height}
                stroke="white"
                strokeWidth={1}
                opacity={0.5}
                strokeDasharray="6 4"
            />

            {/* Horizontal line */}
            <line
                x1={0}
                y1={crispY}
                x2={width}
                y2={crispY}
                stroke="white"
                strokeWidth={1}
                opacity={0.5}
                strokeDasharray="6 4"
            />

            {/* Center marker */}
            <circle
                cx={clampedX}
                cy={clampedY}
                r={4}
                fill="rgba(0,0,0,0.6)"
                stroke="white"
                strokeWidth={1}
                opacity={0.8}
            />
        </g>
    );
};

export default CrossHair;
