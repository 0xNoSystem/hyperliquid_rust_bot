import React, { useRef, useEffect } from "react";
import { priceToY, timeToX } from "./utils";

export interface LinePoint {
    ts: number;
    value: number;
}

export interface LineSeries {
    points: LinePoint[];
    color: string;
    label?: string;
    lineWidth?: number;
}

export interface LineCanvasProps {
    width: number;
    height: number;
    series: LineSeries[];
    startTime: number;
    endTime: number;
    minValue: number;
    maxValue: number;
    className?: string;
    onMouseMove?: (e: React.MouseEvent<HTMLCanvasElement>) => void;
    onMouseEnter?: (e: React.MouseEvent<HTMLCanvasElement>) => void;
    onMouseLeave?: (e: React.MouseEvent<HTMLCanvasElement>) => void;
}

const LineCanvas: React.FC<LineCanvasProps> = ({
    width,
    height,
    series,
    startTime,
    endTime,
    minValue,
    maxValue,
    className,
    onMouseMove,
    onMouseEnter,
    onMouseLeave,
}) => {
    const canvasRef = useRef<HTMLCanvasElement | null>(null);
    const rafRef = useRef<number | null>(null);

    useEffect(() => {
        const canvas = canvasRef.current;
        if (!canvas) return;

        const cssWidth = Math.max(0, width);
        const cssHeight = Math.max(0, height);
        if (cssWidth === 0 || cssHeight === 0) {
            canvas.width = 0;
            canvas.height = 0;
            return;
        }

        const dpr = Math.max(1, window.devicePixelRatio || 1);
        canvas.style.width = `${cssWidth}px`;
        canvas.style.height = `${cssHeight}px`;

        const targetWidth = Math.floor(cssWidth * dpr);
        const targetHeight = Math.floor(cssHeight * dpr);

        if (canvas.width !== targetWidth || canvas.height !== targetHeight) {
            canvas.width = targetWidth;
            canvas.height = targetHeight;
        }

        const ctx = canvas.getContext("2d");
        if (!ctx) return;

        const draw = () => {
            ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
            ctx.clearRect(0, 0, cssWidth, cssHeight);

            if (endTime <= startTime || maxValue <= minValue) return;

            for (const s of series) {
                if (s.points.length === 0) continue;

                if (s.points.length === 1) {
                    const p = s.points[0];
                    const x = timeToX(p.ts, startTime, endTime, cssWidth);
                    const y = priceToY(p.value, minValue, maxValue, cssHeight);
                    ctx.beginPath();
                    ctx.arc(x, y, 3, 0, Math.PI * 2);
                    ctx.fillStyle = s.color;
                    ctx.fill();
                    continue;
                }

                ctx.beginPath();
                ctx.strokeStyle = s.color;
                ctx.lineWidth = s.lineWidth ?? 1.5;
                ctx.lineJoin = "round";

                let started = false;
                for (let i = 0; i < s.points.length; i++) {
                    const p = s.points[i];
                    if (p.ts < startTime || p.ts > endTime) {
                        // draw offscreen edges so lines connect through the viewport
                        if (
                            !started &&
                            i + 1 < s.points.length &&
                            s.points[i + 1].ts >= startTime
                        ) {
                            const x = timeToX(
                                p.ts,
                                startTime,
                                endTime,
                                cssWidth
                            );
                            const y = priceToY(
                                p.value,
                                minValue,
                                maxValue,
                                cssHeight
                            );
                            ctx.moveTo(x, y);
                            started = true;
                        }
                        if (started) {
                            const x = timeToX(
                                p.ts,
                                startTime,
                                endTime,
                                cssWidth
                            );
                            const y = priceToY(
                                p.value,
                                minValue,
                                maxValue,
                                cssHeight
                            );
                            ctx.lineTo(x, y);
                            if (p.ts > endTime) break;
                        }
                        continue;
                    }

                    const x = timeToX(p.ts, startTime, endTime, cssWidth);
                    const y = priceToY(p.value, minValue, maxValue, cssHeight);

                    if (!started) {
                        ctx.moveTo(x, y);
                        started = true;
                    } else {
                        ctx.lineTo(x, y);
                    }
                }

                ctx.stroke();
            }
        };

        if (rafRef.current !== null) {
            cancelAnimationFrame(rafRef.current);
        }
        rafRef.current = requestAnimationFrame(draw);

        return () => {
            if (rafRef.current !== null) {
                cancelAnimationFrame(rafRef.current);
            }
        };
    }, [width, height, series, startTime, endTime, minValue, maxValue]);

    return (
        <canvas
            ref={canvasRef}
            className={className}
            onMouseMove={onMouseMove}
            onMouseEnter={onMouseEnter}
            onMouseLeave={onMouseLeave}
        />
    );
};

export default LineCanvas;
