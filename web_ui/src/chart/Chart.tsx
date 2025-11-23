import { useRef, useEffect, useState } from "react";
import type { TimeFrame } from "../types";
import Candle from "./visual/Candle";
import CrossHair from "./visual/CrossHair";

export interface ChartProps {
    asset: String;
    tf: TimeFrame;
    settingInterval: bool;
}

const Chart: React.FC<ChartProps> = ({ asset, tf, settingInterval}) => {
    const ref = useRef<HTMLDivElement>(null);
    const [size, setSize] = useState({ width: 0, height: 0 });
    const [mousePos, setMousePos] = useState<{ x: number; y: number }>({
        x: 0,
        y: 0,
    });

    const handleMouseMove = (e: React.MouseEvent<SVGSVGElement>) => {
        const rect = e.currentTarget.getBoundingClientRect();

        const x = e.clientX - rect.left;
        const y = e.clientY - rect.top;

        setMousePos({ x, y });
    };

    const [isInside, setIsInside] = useState(false);

    const handleMouseEnter = () => setIsInside(true);
    const handleMouseLeave = () => setIsInside(false);

    useEffect(() => {
        if (!ref.current) return;
        const observer = new ResizeObserver(([entry]) => {
            const { width, height } = entry.contentRect;
            setSize({ width, height });
        });
        observer.observe(ref.current);
        return () => observer.disconnect();
    }, []);

    return (
        <div ref={ref} className="relative h-full w-full flex-1">
            <svg
                width={size.width}
                height={size.height}
                onMouseMove={handleMouseMove}
                onMouseEnter={handleMouseEnter}
                onMouseLeave={handleMouseLeave}
                className="min-h-full min-w-full"
            >
                <Candle
                    height={size.height / 10}
                    width={size.width / 120}
                    color="green"
                />
                <Candle height={10} width={0.5} color="red" />

                {(isInside && !settingInterval) && (
                    <CrossHair
                        x={mousePos.x}
                        y={mousePos.y}
                        height={size.height}
                        width={size.width}
                        color="white"
                    />
                )}
            </svg>
        </div>
    );
};

export default Chart;
