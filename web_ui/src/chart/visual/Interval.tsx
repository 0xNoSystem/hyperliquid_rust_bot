import React, { useEffect, useMemo } from "react";
import { useChartContext } from "../ChartContext";
import { timeToX } from "../utils";

const MIN_WINDOW_RATIO = 0.04; // 2% of visible range
const MIN_WINDOW_MS = 60 * 1000; // 1 minute fallback

const clamp = (value: number, min: number, max: number) => {
    return Math.min(Math.max(value, min), max);
};

const IntervalOverlay: React.FC = () => {
    const {
        width,
        height,
        startTime,
        endTime,
        selectingInterval,
        intervalStartX,
        intervalEndX,
        setIntervalStartX,
        setIntervalEndX,
    } = useChartContext();

    const effectiveEndTime = Math.min(endTime, Date.now());
    const rangeMs = effectiveEndTime - startTime;
    const minWindow = useMemo(() => {
        if (rangeMs <= 0) return MIN_WINDOW_MS;
        return Math.min(
            rangeMs,
            Math.max(MIN_WINDOW_MS, rangeMs * MIN_WINDOW_RATIO)
        );
    }, [rangeMs]);

    // Ensure we have an interval defined whenever selection mode is active.
    useEffect(() => {
        if (!selectingInterval) return;
        if (rangeMs <= 0) return;

        let start = intervalStartX ?? startTime + rangeMs * 0.2;
        let end = intervalEndX ?? startTime + rangeMs * 0.8;

        let changed = false;

        if (start < startTime) {
            start = startTime;
            changed = true;
        }
        if (end > effectiveEndTime) {
            end = effectiveEndTime;
            changed = true;
        }
        if (end - start < minWindow) {
            end = Math.min(effectiveEndTime, start + minWindow);
            start = Math.max(startTime, end - minWindow);
            changed = true;
        }

        if (changed || intervalStartX === null || intervalEndX === null) {
            setIntervalStartX(start);
            setIntervalEndX(end);
        }
    }, [
        selectingInterval,
        rangeMs,
        startTime,
        endTime,
        minWindow,
        intervalStartX,
        intervalEndX,
        setIntervalStartX,
        setIntervalEndX,
    ]);

    if (!selectingInterval || width <= 0 || rangeMs <= 0) return null;
    if (intervalStartX === null || intervalEndX === null) return null;

    let start = clamp(intervalStartX, startTime, effectiveEndTime);
    let end = clamp(intervalEndX, startTime, effectiveEndTime);
    if (end - start < minWindow) {
        end = Math.min(effectiveEndTime, start + minWindow);
        start = Math.max(startTime, end - minWindow);
    }

    const left = timeToX(start, startTime, endTime, width);
    const right = timeToX(end, startTime, endTime, width);
    const overlayWidth = Math.max(10, right - left);

    const msPerPx = width > 0 ? rangeMs / width : 0;

    const beginDrag =
        (mode: "move" | "start" | "end") => (e: React.MouseEvent) => {
            e.preventDefault();
            e.stopPropagation();

            const startClientX = e.clientX;
            const initialStart = start;
            const initialEnd = end;
            const windowSize = initialEnd - initialStart;

            const handleMove = (ev: MouseEvent) => {
                const dx = ev.clientX - startClientX;
                const dt = dx * msPerPx;

                if (mode === "move") {
                    let nextStart = initialStart + dt;
                    let nextEnd = initialEnd + dt;

                    if (nextStart < startTime) {
                        const offset = startTime - nextStart;
                        nextStart += offset;
                        nextEnd += offset;
                    }
                    if (nextEnd > effectiveEndTime) {
                        const offset = nextEnd - effectiveEndTime;
                        nextStart -= offset;
                        nextEnd -= offset;
                    }

                    setIntervalStartX(
                        clamp(
                            nextStart,
                            startTime,
                            effectiveEndTime - minWindow
                        )
                    );
                    setIntervalEndX(
                        clamp(nextEnd, startTime + minWindow, effectiveEndTime)
                    );
                    return;
                }

                if (mode === "start") {
                    let nextStart = clamp(
                        initialStart + dt,
                        startTime,
                        initialEnd - minWindow
                    );
                    setIntervalStartX(nextStart);
                    return;
                }

                let nextEnd = clamp(
                    initialEnd + dt,
                    initialStart + minWindow,
                    effectiveEndTime
                );
                setIntervalEndX(nextEnd);
            };

            const handleUp = () => {
                window.removeEventListener("mousemove", handleMove);
                window.removeEventListener("mouseup", handleUp);
            };

            window.addEventListener("mousemove", handleMove);
            window.addEventListener("mouseup", handleUp);
        };

    return (
        <div className="pointer-events-none absolute inset-0">
            <div
                className="pointer-events-auto absolute top-0 flex h-full cursor-grab items-stretch border-2 border-b-0 border-orange-400/60 bg-orange-500/15"
                style={{ left, width: overlayWidth }}
                onMouseDown={beginDrag("move")}
            >
                <div className="pointer-events-none absolute w-[100%] overflow-hidden py-1 text-xs font-semibold text-black">
                    <span className="bg-orange-500/80 p-2">BackTest</span>
                </div>
                <button
                    className="pointer-events-auto absolute top-1/2 left-0 h-8 w-4 -translate-x-2 -translate-y-1/2 cursor-w-resize rounded-full border border-white/70 bg-black hover:bg-orange-500"
                    onMouseDown={beginDrag("start")}
                    title="Adjust start"
                />
                <button
                    className="pointer-events-auto absolute top-1/2 right-0 h-8 w-4 translate-x-2 -translate-y-1/2 cursor-w-resize rounded-full border border-white/70 bg-black hover:bg-orange-500"
                    onMouseDown={beginDrag("end")}
                    title="Adjust end"
                />
            </div>
        </div>
    );
};

export default IntervalOverlay;
