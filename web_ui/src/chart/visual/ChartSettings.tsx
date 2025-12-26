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
        <div className="w-64 rounded-2xl border border-line-panel bg-surface-panel p-4 text-app-text shadow-lg">
            <h2 className="mb-4 border-b text-lg font-semibold text-muted">
                Settings
            </h2>

            <div className="flex flex-col gap-3">
                <h3 className="">Candle Color</h3>
                <div className="flex items-center justify-between">
                    <label htmlFor="upColor" className="text-sm text-faint">
                        Up
                    </label>
                    <input
                        id="upColor"
                        type="text"
                        value={colors.up}
                        onChange={(e) => handleChange("up", e.target.value)}
                        placeholder="#00ff00"
                        className="h-8 w-28 rounded border border-line-panel-strong bg-transparent px-2 text-sm text-muted focus:ring-1 focus:ring-chart-action-hover focus:outline-none"
                    />
                </div>

                <div className="flex items-center justify-between">
                    <label htmlFor="downColor" className="text-sm text-faint">
                        Down
                    </label>
                    <input
                        id="downColor"
                        type="text"
                        value={colors.down}
                        onChange={(e) => handleChange("down", e.target.value)}
                        placeholder="#ff0000"
                        className="h-8 w-28 rounded border border-line-panel-strong bg-transparent px-2 text-sm text-muted focus:ring-1 focus:ring-chart-action-hover focus:outline-none"
                    />
                </div>

                <div className="mt-4 flex justify-between">
                    <button
                        onClick={handleReset}
                        className="rounded-md border border-line-panel-soft px-3 py-1 text-sm transition hover:bg-surface-panel-strong"
                    >
                        Reset
                    </button>
                    <button
                        onClick={handleApply}
                        className="rounded-md bg-chart-action-bg px-3 py-1 text-sm transition hover:bg-chart-action-hover"
                    >
                        OK
                    </button>
                </div>
            </div>
        </div>
    );
};

export default ChartSettings;
