import React from "react";

interface CandleProps {
    x: number;
    width: number;

    bodyTop: number;
    bodyHeight: number;

    wickTop: number;
    wickHeight: number;

    color: string;
}

const Candle: React.FC<CandleProps> = ({
    x,
    width,
    bodyTop,
    bodyHeight,
    wickTop,
    wickHeight,
    color,
}) => {
    const centerX = x + width / 2;

    return (
        <g>
            {/* Wick */}
            <line
                x1={centerX}
                y1={wickTop}
                x2={centerX}
                y2={wickTop + wickHeight}
                stroke={color}
                strokeWidth={ (width / 2 <= 1) ? 0.2 : 1}
            />

            {/* Body */}
            <rect
                x={x}
                y={bodyTop}
                width={width}
                height={bodyHeight === 0 ? 1 : bodyHeight}
                fill={color}
            />
        </g>
    );
};

export default Candle;
