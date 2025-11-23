import React from "react";

interface CandleProps {
    width: number;
    height: number;
    color: string;
}

const Candle: React.FC<CandleProps> = ({ width, height, color }) => {
    return (
        <g pointerEvents="none">
            <rect
                x={10}
                y={10}
                width={width}
                height={height}
                fill={color}
                stroke="black"
                strokeWidth={0.1}
                rx={1}
                ry={1}
            />
        </g>
    );
};

export default Candle;
