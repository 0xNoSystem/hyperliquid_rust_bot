import React from "react";

interface CrossHairProps {
    x: number;
    y: number;
    width: number;
    height: number;
    color?: string;
}

const CrossHair: React.FC<CrossHairProps> = ({
    x,
    y,
    width,
    height,
    color = "white",
}) => {
    return (
        <g pointerEvents="none" className="z-10">
            {/* Vertical line */}
            <line
                x1={x}
                y1={0}
                x2={x}
                y2={height}
                stroke={color}
                strokeWidth={1}
                opacity={0.5}
                strokeDasharray="6 4"
            />

            {/* Horizontal line */}
            <line
                x1={0}
                y1={y}
                x2={width}
                y2={y}
                stroke={color}
                strokeWidth={1}
                opacity={0.5}
                strokeDasharray="6 4"
            />
        </g>
    );
};

export default CrossHair;
