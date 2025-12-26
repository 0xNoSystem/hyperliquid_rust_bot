import React, { useState } from "react";

type CandleColor = {
    up: string;
    down: string;
};

interface ChartSettingsProps {
    initialColors?: CandleColor;
    onApply?: (colors: CandleColor) => void;
    onReset?: () => void;
    onClose?: () => void;
}

const ChartSettings: React.FC<ChartSettingsProps> = ({
    initialColors = { up: "#cf7b15", down: "#c4c3c2" },
    onApply,
    onReset,
    onClose,
}) => {
    const [colors, setColors] = useState<CandleColor>(initialColors);

    const handleChange = (key: keyof CandleColor, value: string) => {
        setColors((prev) => ({ ...prev, [key]: value }));
    };

    const handleApply = () => {
        onApply?.(colors);
        onClose?.();
    };

    const handleReset = () => {
        setColors({ up: "#cf7b15", down: "#c4c3c2" });
        onReset?.();
    };

    return (
        <div className="border-line-panel bg-surface-panel text-app-text w-64 rounded-2xl border p-4 shadow-lg">
            <h2 className="text-muted mb-4 border-b text-lg font-semibold">
                Settings
            </h2>

            <div className="flex flex-col gap-3">
                <h3 className="">Candle Color</h3>
                <div className="flex items-center justify-between">
                    <label htmlFor="upColor" className="text-faint text-sm">
                        Up
                    </label>
                    <input
                        id="upColor"
                        type="text"
                        value={colors.up}
                        onChange={(e) => handleChange("up", e.target.value)}
                        placeholder="#00ff00"
                        className="border-line-panel-strong text-muted focus:ring-chart-action-hover h-8 w-28 rounded border bg-transparent px-2 text-sm focus:ring-1 focus:outline-none"
                    />
                </div>

                <div className="flex items-center justify-between">
                    <label htmlFor="downColor" className="text-faint text-sm">
                        Down
                    </label>
                    <input
                        id="downColor"
                        type="text"
                        value={colors.down}
                        onChange={(e) => handleChange("down", e.target.value)}
                        placeholder="#ff0000"
                        className="border-line-panel-strong text-muted focus:ring-chart-action-hover h-8 w-28 rounded border bg-transparent px-2 text-sm focus:ring-1 focus:outline-none"
                    />
                </div>

                <div className="mt-4 flex justify-between">
                    <button
                        onClick={handleReset}
                        className="border-line-panel-soft hover:bg-surface-panel-strong rounded-md border px-3 py-1 text-sm transition"
                    >
                        Reset
                    </button>
                    <button
                        onClick={handleApply}
                        className="bg-chart-action-bg hover:bg-chart-action-hover rounded-md px-3 py-1 text-sm transition"
                    >
                        OK
                    </button>
                </div>
            </div>
        </div>
    );
};

export default ChartSettings;
