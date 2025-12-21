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
        <div className="w-64 rounded-2xl border border-neutral-800 bg-[#1E1E1E] p-4 text-white shadow-lg">
            <h2 className="mb-4 border-b text-lg font-semibold text-gray-200">
                Settings
            </h2>

            <div className="flex flex-col gap-3">
                <h3 className="">Candle Color</h3>
                <div className="flex items-center justify-between">
                    <label htmlFor="upColor" className="text-sm text-gray-400">
                        Up
                    </label>
                    <input
                        id="upColor"
                        type="text"
                        value={colors.up}
                        onChange={(e) => handleChange("up", e.target.value)}
                        placeholder="#00ff00"
                        className="h-8 w-28 rounded border border-neutral-700 bg-transparent px-2 text-sm text-gray-200 focus:ring-1 focus:ring-blue-500 focus:outline-none"
                    />
                </div>

                <div className="flex items-center justify-between">
                    <label
                        htmlFor="downColor"
                        className="text-sm text-gray-400"
                    >
                        Down
                    </label>
                    <input
                        id="downColor"
                        type="text"
                        value={colors.down}
                        onChange={(e) => handleChange("down", e.target.value)}
                        placeholder="#ff0000"
                        className="h-8 w-28 rounded border border-neutral-700 bg-transparent px-2 text-sm text-gray-200 focus:ring-1 focus:ring-blue-500 focus:outline-none"
                    />
                </div>

                <div className="mt-4 flex justify-between">
                    <button
                        onClick={handleReset}
                        className="rounded-md border border-neutral-600 px-3 py-1 text-sm transition hover:bg-neutral-700"
                    >
                        Reset
                    </button>
                    <button
                        onClick={handleApply}
                        className="rounded-md bg-blue-600 px-3 py-1 text-sm transition hover:bg-blue-500"
                    >
                        OK
                    </button>
                </div>
            </div>
        </div>
    );
};

export default ChartSettings;
