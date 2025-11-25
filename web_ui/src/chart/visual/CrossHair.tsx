import React from "react";
import { useChartContext } from "../ChartContext";

const CrossHair = ({}) => {
    const { crosshairX, crosshairY, height, width } = useChartContext();
    return (
        <g pointerEvents="none" className="z-10">
            {/* Vertical line */}
            <line
                x1={crosshairX}
                y1={0}
                x2={crosshairX}
                y2={height}
                stroke="white"
                strokeWidth={1}
                opacity={0.5}
                strokeDasharray="6 4"
            />

            {/* Horizontal line */}
            <line
                x1={0}
                y1={crosshairY}
                x2={width}
                y2={crosshairY}
                stroke="white"
                strokeWidth={1}
                opacity={0.5}
                strokeDasharray="6 4"
            />
        </g>
    );
};

export default CrossHair;
