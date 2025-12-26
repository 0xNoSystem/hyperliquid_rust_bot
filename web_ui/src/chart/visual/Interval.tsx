import React, { useCallback, useEffect, useMemo } from "react";
import { useChartContext } from "../ChartContextStore";
import { timeToX } from "../utils";
import { TF_TO_MS } from "../../types";

const MIN_WINDOW_RATIO = 0.04; // 2% of visible range
const MIN_WINDOW_MS = 60 * 1000; // 1 minute fallback

const clamp = (value: number, min: number, max: number) => {
    return Math.min(Math.max(value, min), max);
};

const getMonthIndex = (timeMs: number) => {
    const d = new Date(timeMs);
    return d.getUTCFullYear() * 12 + d.getUTCMonth();
};

const getMonthCenter = (monthIndex: number) => {
    const year = Math.floor(monthIndex / 12);
    const month = monthIndex % 12;
    const start = Date.UTC(year, month, 1);
    const next = Date.UTC(year, month + 1, 1);
    return start + (next - start) / 2;
};

const snapToMonthCenter = (timeMs: number) => {
    const baseIndex = getMonthIndex(timeMs);
    const candidates = [baseIndex - 1, baseIndex, baseIndex + 1];
    let best = getMonthCenter(candidates[0]);
    let bestDiff = Math.abs(best - timeMs);
    for (let i = 1; i < candidates.length; i++) {
        const center = getMonthCenter(candidates[i]);
        const diff = Math.abs(center - timeMs);
        if (diff < bestDiff) {
            bestDiff = diff;
            best = center;
        }
    }
    return best;
};

const IntervalOverlay: React.FC = () => {
    const {
        width,
        startTime,
        endTime,
        selectingInterval,
        intervalStartX,
        intervalEndX,
        setIntervalStartX,
        setIntervalEndX,
        timeframe,
        candles,
    } = useChartContext();

    const visibleRangeMs = endTime - startTime;
    const lastCandleEnd =
        candles.length > 0 ? candles[candles.length - 1].end : endTime;
    const maxTime = Math.min(endTime, lastCandleEnd);
    const minSelectableTime = candles.length > 0 ? candles[0].start : startTime;
    const minBound = Math.max(startTime, minSelectableTime);
    const stepMs = timeframe ? (TF_TO_MS[timeframe] ?? 0) : 0;
    const stepOrigin =
        stepMs > 0
            ? (candles.length > 0 ? candles[0].start : startTime) + stepMs / 2
            : 0;
    const snapTime = useCallback(
        (timeMs: number) => {
            if (!timeframe || stepMs <= 0) return timeMs;
            if (timeframe === "month") return snapToMonthCenter(timeMs);
            const idx = Math.round((timeMs - stepOrigin) / stepMs);
            return stepOrigin + idx * stepMs;
        },
        [stepMs, stepOrigin, timeframe]
    );
    const maxSelectableTime = useMemo(() => {
        if (!timeframe || stepMs <= 0) {
            return Math.max(minSelectableTime, maxTime);
        }
        if (timeframe === "month") {
            const maxMonthIndex = getMonthIndex(maxTime);
            let center = getMonthCenter(maxMonthIndex);
            if (center > maxTime) {
                center = getMonthCenter(maxMonthIndex - 1);
            }
            return Math.max(minSelectableTime, center);
        }
        const idx = Math.floor((maxTime - stepOrigin) / stepMs);
        return Math.max(minSelectableTime, stepOrigin + idx * stepMs);
    }, [maxTime, minSelectableTime, stepMs, stepOrigin, timeframe]);

    const minWindow = useMemo(() => {
        if (visibleRangeMs <= 0) return MIN_WINDOW_MS;
        return Math.min(
            visibleRangeMs,
            Math.max(MIN_WINDOW_MS, visibleRangeMs * MIN_WINDOW_RATIO)
        );
    }, [visibleRangeMs]);

    // Ensure we have an interval defined whenever selection mode is active.
    useEffect(() => {
        if (!selectingInterval) return;
        if (visibleRangeMs <= 0) return;

        let start = intervalStartX ?? startTime + visibleRangeMs * 0.2;
        let end = intervalEndX ?? startTime + visibleRangeMs * 0.8;

        start = clamp(snapTime(start), minBound, maxSelectableTime);
        end = clamp(snapTime(end), minBound, maxSelectableTime);

        if (end - start < minWindow) {
            end = Math.min(maxSelectableTime, start + minWindow);
            start = Math.max(minBound, end - minWindow);
            start = clamp(snapTime(start), minBound, maxSelectableTime);
            end = clamp(snapTime(end), minBound, maxSelectableTime);
        }

        if (
            intervalStartX === null ||
            intervalEndX === null ||
            start !== intervalStartX ||
            end !== intervalEndX
        ) {
            setIntervalStartX(start);
            setIntervalEndX(end);
        }
    }, [
        selectingInterval,
        visibleRangeMs,
        startTime,
        endTime,
        minWindow,
        intervalStartX,
        intervalEndX,
        setIntervalStartX,
        setIntervalEndX,
        maxSelectableTime,
        minBound,
        snapTime,
    ]);

    if (!selectingInterval || width <= 0 || visibleRangeMs <= 0) return null;
    if (intervalStartX === null || intervalEndX === null) return null;

    let start = clamp(snapTime(intervalStartX), minBound, maxSelectableTime);
    let end = clamp(snapTime(intervalEndX), minBound, maxSelectableTime);
    if (end - start < minWindow) {
        end = Math.min(maxSelectableTime, start + minWindow);
        start = Math.max(minBound, end - minWindow);
    }

    const left = timeToX(start, startTime, endTime, width);
    const right = timeToX(end, startTime, endTime, width);
    const crispLeft = Math.round(left) + 0.5;
    const crispRight = Math.round(right) + 0.5;
    const overlayLeft = Math.min(crispLeft, crispRight);
    const overlayWidth = Math.max(1, Math.abs(crispRight - crispLeft));

    const msPerPx = width > 0 ? visibleRangeMs / width : 0;

    const beginDrag =
        (mode: "move" | "start" | "end") => (e: React.MouseEvent) => {
            e.preventDefault();
            e.stopPropagation();

            const startClientX = e.clientX;
            const initialStart = start;
            const initialEnd = end;

            const handleMove = (ev: MouseEvent) => {
                const dx = ev.clientX - startClientX;
                const dt = dx * msPerPx;

                if (mode === "move") {
                    let nextStart = initialStart + dt;
                    let nextEnd = initialEnd + dt;

                    if (timeframe && stepMs > 0) {
                        if (timeframe === "month") {
                            const startMonth = getMonthIndex(initialStart);
                            const endMonth = getMonthIndex(initialEnd);
                            const targetStart = snapToMonthCenter(
                                initialStart + dt
                            );
                            const targetMonth = getMonthIndex(targetStart);
                            const delta = targetMonth - startMonth;
                            nextStart = getMonthCenter(startMonth + delta);
                            nextEnd = getMonthCenter(endMonth + delta);
                        } else {
                            const startIdx = Math.round(
                                (initialStart - stepOrigin) / stepMs
                            );
                            const targetIdx = Math.round(
                                (initialStart + dt - stepOrigin) / stepMs
                            );
                            const shift = (targetIdx - startIdx) * stepMs;
                            nextStart = initialStart + shift;
                            nextEnd = initialEnd + shift;
                        }
                    }

                    if (nextStart < minBound) {
                        const offset = minBound - nextStart;
                        nextStart += offset;
                        nextEnd += offset;
                    }
                    if (nextEnd > maxSelectableTime) {
                        const offset = nextEnd - maxSelectableTime;
                        nextStart -= offset;
                        nextEnd -= offset;
                    }

                    setIntervalStartX(
                        clamp(
                            nextStart,
                            minBound,
                            maxSelectableTime - minWindow
                        )
                    );
                    setIntervalEndX(
                        clamp(nextEnd, minBound + minWindow, maxSelectableTime)
                    );
                    return;
                }

                if (mode === "start") {
                    let nextStart = clamp(
                        initialStart + dt,
                        minBound,
                        initialEnd - minWindow
                    );
                    nextStart = snapTime(nextStart);
                    nextStart = clamp(
                        nextStart,
                        minBound,
                        initialEnd - minWindow
                    );
                    setIntervalStartX(nextStart);
                    return;
                }

                let nextEnd = clamp(
                    initialEnd + dt,
                    initialStart + minWindow,
                    maxSelectableTime
                );
                nextEnd = snapTime(nextEnd);
                nextEnd = clamp(
                    nextEnd,
                    initialStart + minWindow,
                    maxSelectableTime
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
                className="pointer-events-auto absolute top-0 flex h-full cursor-grab items-stretch bg-chart-selection-bg"
                style={{ left: overlayLeft, width: overlayWidth }}
                onMouseDown={beginDrag("move")}
            >
                <div
                    className="pointer-events-none absolute top-0 h-full w-px bg-chart-selection-edge"
                    style={{ left: 0, transform: "translateX(-0.5px)" }}
                />
                <div
                    className="pointer-events-none absolute top-0 h-full w-px bg-chart-selection-edge"
                    style={{ left: "100%", transform: "translateX(-0.5px)" }}
                />
                <div className="pointer-events-none absolute w-[100%] overflow-hidden py-1 text-xs font-semibold text-on-bright">
                    <span className="bg-chart-selection-label-bg p-2">
                        BackTest
                    </span>
                </div>
                <button
                    className="pointer-events-auto absolute top-1/2 left-0 h-8 w-4 -translate-x-2 -translate-y-1/2 cursor-w-resize rounded-full border border-line-stronger bg-chart-selection-handle-bg hover:bg-chart-selection-handle-hover"
                    onMouseDown={beginDrag("start")}
                    title="Adjust start"
                />
                <button
                    className="pointer-events-auto absolute top-1/2 right-0 h-8 w-4 translate-x-2 -translate-y-1/2 cursor-w-resize rounded-full border border-line-stronger bg-chart-selection-handle-bg hover:bg-chart-selection-handle-hover"
                    onMouseDown={beginDrag("end")}
                    title="Adjust end"
                />
            </div>
        </div>
    );
};

export default IntervalOverlay;
