import React, { useState, useMemo } from "react";
import {
    into,
    TIMEFRAME_CAMELCASE,
    indicatorLabels,
    indicatorColors,
    indicatorParamLabels,
} from "../types";
import type {
    TimeFrame,
    Strategy,
    TradeParams,
    AddMarketInfo,
    IndexId,
    IndicatorKind,
    AddMarketProps,
} from "../types";

const strategyOptions: Strategy[] = ["rsiEmaScalp"];
const indicatorKinds: IndicatorKind[] = [
    "rsi",
    "smaOnRsi",
    "stochRsi",
    "adx",
    "atr",
    "ema",
    "emaCross",
    "sma",
];

function getMaxLeverage(name: string): number | undefined {
    return assets.find((u) => u.name === name)?.maxLeverage;
}

export const AddMarket: React.FC<AddMarketProps> = ({
    onClose,
    totalMargin,
    assets,
}) => {
    const [asset, setAsset] = useState("");
    const [marginType, setMarginType] = useState<"alloc" | "amount">("alloc");
    const [marginValue, setMarginValue] = useState(0.1);
    const [lev, setLev] = useState(1);
    const [strategy, setStrategy] = useState<Strategy>("rsiEmaScalp");

    const [showConfig, setShowConfig] = useState(false);
    const [config, setConfig] = useState<IndexId[]>([]);

    const [newKind, setNewKind] = useState<IndicatorKind>("rsi");
    const [newParam, setNewParam] = useState(14);
    const [newParam2, setNewParam2] = useState(14);
    const [newTf, setNewTf] = useState<keyof typeof TIMEFRAME_CAMELCASE>("1m");

    const computedAmount = useMemo(
        () => (marginType === "alloc" ? totalMargin * (marginValue / 100) : 0),
        [marginType, marginValue, totalMargin]
    );

    const handleAddIndicator = () => {
        let cfg: any;
        switch (newKind) {
            case "emaCross":
                cfg = { emaCross: { short: newParam, long: newParam2 } };
                break;
            case "smaOnRsi":
                cfg = {
                    smaOnRsi: {
                        periods: newParam,
                        smoothing_length: newParam2,
                    },
                };
                break;
            case "stochRsi":
                cfg = {
                    stochRsi: {
                        periods: newParam,
                        k_smoothing: null,
                        d_smoothing: null,
                    },
                };
                break;
            case "adx":
                cfg = { adx: { periods: newParam, di_length: newParam2 } };
                break;
            default:
                cfg = { [newKind]: newParam };
        }

        const newItem: [any, string] = [cfg, newTf];

        setConfig((prev) => {
            const exists = prev.some(
                (item) => JSON.stringify(item) === JSON.stringify(newItem)
            );
            return exists ? prev : [...prev, newItem];
        });

        setShowConfig(false);
    };

    const handleRemove = (i: number) =>
        setConfig(config.filter((_, idx) => idx !== i));

    const handleSubmit = async (e: React.FormEvent) => {
        e.preventDefault();
        const validConfig = config.map(([ind, tf]) => [ind, into(tf)]);
        const info: AddMarketInfo = {
            asset: asset,
            marginAlloc:
                marginType === "alloc"
                    ? { alloc: marginValue / 100 }
                    : { amount: marginValue },
            tradeParams: {
                timeFrame: "min1",
                lev,
                strategy,
                tradeTime: 100,
            }as TradeParams,
            config: validConfig,
        };

        console.log(JSON.stringify(validConfig));

        const res = await fetch("http://127.0.0.1:8090/command", {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ addMarket: info }),
        });
        if (res.ok) onClose();
        else console.error("Submit failed");
    };

    const inputClass =
        "mt-1 w-full border border-white bg-gray-600 text-white rounded px-3 py-2";
    const selectClass =
        "mt-1 w-full border border-white bg-gray-600 text-white rounded px-3 py-2 cursor-pointer";
    const btnClass =
        "px-5 py-2 border border-white bg-gray-600 text-white rounded hover:bg-gray-500 cursor-pointer";

    return (
        <div className="fixed inset-0 z-50 flex scale-[0.88] transform items-center justify-center backdrop-blur-sm">
            <form
                onSubmit={handleSubmit}
                className="relative w-full max-w-lg scale-90 space-y-6 rounded-2xl bg-gray-600 p-8 shadow-2xl"
            >
                <h2 className="text-2xl font-bold text-white">
                    Add New Market
                </h2>
                <div className="text-sm text-white">
                    Available Margin:{" "}
                    <span className="font-semibold">
                        {totalMargin.toFixed(2)}
                    </span>
                </div>
                <div className="grid grid-cols-2 gap-4">
                    <div className="col-span-2">
                        <label className="block text-sm text-white">
                            Asset Symbol
                        </label>
                        <select
                            value={asset}
                            onChange={(e) => setAsset(e.target.value)}
                            required
                            className={`${inputClass} bg-gray-700 text-white`}
                        >
                            <option value="" disabled>
                                -- select an asset --
                            </option>
                            {assets.map((u) => (
                                <option key={u.name} value={u.name}>
                                    {u.name}
                                </option>
                            ))}
                        </select>
                    </div>
                    <div>
                        <label className="block text-sm text-white">
                            Margin Type
                        </label>
                        <select
                            value={marginType}
                            onChange={(e) =>
                                setMarginType(e.target.value as any)
                            }
                            className={selectClass}
                        >
                            <option value="alloc">Percent</option>
                            <option value="amount">Fixed</option>
                        </select>
                    </div>
                    <div className="col-span-2">
                        <label className="block text-sm text-white">
                            {marginType === "alloc" ? "Margin %" : "Value"}
                        </label>
                        {marginType === "alloc" ? (
                            <>
                                <input
                                    type="range"
                                    min={0}
                                    max={100}
                                    step={0.1}
                                    value={marginValue}
                                    onChange={(e) =>
                                        setMarginValue(+e.target.value)
                                    }
                                    className="h-2 w-full cursor-pointer bg-gray-200"
                                />
                                <div className="mt-1 flex justify-between text-sm text-white">
                                    <span>0%</span>
                                    <span>{marginValue.toFixed(1)}%</span>
                                    <span>100%</span>
                                </div>
                                <div className="text-sm text-white">
                                    Eq: {computedAmount.toFixed(2)}
                                </div>
                            </>
                        ) : (
                            <input
                                type="number"
                                step="any"
                                value={marginValue}
                                onChange={(e) =>
                                    setMarginValue(+e.target.value)
                                }
                                className={inputClass}
                            />
                        )}
                    </div>
                    <div className="col-span-2">
                        <label className="block text-center text-sm text-white">
                            Leverage: {lev} (MAX:{" "}
                            {assets.find((u) => u.name === asset)?.maxLeverage})
                        </label>
                        <input
                            type="range"
                            min={1}
                            max={
                                assets.find((u) => u.name === asset)
                                    ?.maxLeverage
                            }
                            step={1}
                            value={lev}
                            onChange={(e) => setLev(+e.target.value)}
                            className="h-2 w-full cursor-pointer appearance-none rounded-lg bg-gray-200 bg-no-repeat [&::-moz-range-thumb]:h-4 [&::-moz-range-thumb]:w-4 [&::-moz-range-thumb]:rounded-full [&::-moz-range-thumb]:bg-black [&::-webkit-slider-thumb]:h-4 [&::-webkit-slider-thumb]:w-4 [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-orange-600"
                            style={{
                                background: `linear-gradient(to right, white 0%, red ${
                                    ((lev - 1) /
                                        ((assets.find((u) => u.name === asset)
                                            ?.maxLeverage ?? 1) -
                                            1)) *
                                    100
                                }%, #e5e7eb ${
                                    ((lev - 1) /
                                        ((assets.find((u) => u.name === asset)
                                            ?.maxLeverage ?? 1) -
                                            1)) *
                                    100
                                }%, #e5e7eb 100%)`,
                            }}
                        />
                    </div>
                                    </div>
                <fieldset className="border-t border-white pt-4">
                    <legend className="text-lg text-white">Strategy</legend>
                    <div>
                    <label className="block text-sm text-white">Strategy</label>
                    <select
                        value={strategy}
                        onChange={(e) => setStrategy(e.target.value as Strategy)}
                        className={selectClass}
                    >
                        {strategyOptions.map((s) => (
                            <option key={s} value={s}>{s}</option>
                        ))}
                    </select>
                </div>                </fieldset>
                <fieldset className="relative mt-6 border-t border-white pt-6">
                    <legend className="text-lg text-white">Indicators</legend>
                    <div className="flex flex-col gap-2">
                        <div className="flex flex-wrap">
                            {config.map(([ind, tf], i) => {
                                const kind = Object.keys(
                                    ind
                                )[0] as IndicatorKind;
                                return (
                                    <div
                                        key={i}
                                        className="mb-3 ml-2 flex items-center"
                                    >
                                        <span
                                            className={`${indicatorColors[kind]} rounded-full px-3 py-1 text-xs`}
                                        >
                                            {indicatorLabels[kind] || kind} --{" "}
                                            {tf}
                                        </span>
                                        <button
                                            type="button"
                                            onClick={() => handleRemove(i)}
                                            className="cursor-pointer text-red-600"
                                        >
                                            ×
                                        </button>
                                    </div>
                                );
                            })}
                        </div>
                        <button
                            type="button"
                            onClick={() => setShowConfig(true)}
                            className="mt-2 cursor-pointer text-sm font-bold text-white hover:underline"
                        >
                            Add Indicator
                        </button>
                    </div>
                    {showConfig && (
                        <div className="absolute bottom-10 left-full z-20 ml-4 w-64 rounded border border-white bg-gray-800 p-4 shadow">
                            <h3 className="text-sm font-semibold text-white">
                                New Indicator
                            </h3>
                            <select
                                value={newKind}
                                onChange={(e) =>
                                    setNewKind(e.target.value as IndicatorKind)
                                }
                                className={selectClass}
                            >
                                {indicatorKinds.map((k) => (
                                    <option key={k} value={k}>
                                        {indicatorLabels[k]}
                                    </option>
                                ))}
                            </select>
                            <div className="mt-2 mb-6 flex grid grid-cols-2 flex-col gap-2">
                                {["emaCross", "smaOnRsi", "adx"].includes(
                                    newKind
                                ) ? (
                                    <>
                                        {" "}
                                        <label className="mt-2 text-right">
                                            {indicatorParamLabels[newKind][0]}
                                        </label>
                                        <input
                                            type="number"
                                            value={newParam}
                                            onChange={(e) =>
                                                setNewParam(+e.target.value)
                                            }
                                            placeholder="Param1"
                                            className={inputClass}
                                        />
                                        <label className="mt-2 text-right">
                                            {indicatorParamLabels[newKind][1]}
                                        </label>
                                        <input
                                            type="number"
                                            value={newParam2}
                                            onChange={(e) =>
                                                setNewParam2(+e.target.value)
                                            }
                                            placeholder="Param2"
                                            className={inputClass}
                                        />{" "}
                                    </>
                                ) : (
                                    <>
                                        {" "}
                                        <label className="mt-2 text-right">
                                            {indicatorParamLabels[newKind][0]}
                                        </label>
                                        <input
                                            type="number"
                                            value={newParam}
                                            onChange={(e) =>
                                                setNewParam(+e.target.value)
                                            }
                                            className={inputClass}
                                        />
                                    </>
                                )}
                            </div>
                            <label>Time Frame</label>
                            <select
                                value={newTf}
                                onChange={(e) =>
                                    setNewTf(e.target.value as any)
                                }
                                className={selectClass}
                            >
                                {Object.keys(TIMEFRAME_CAMELCASE).map((t) => (
                                    <option key={t} value={t}>
                                        {t}
                                    </option>
                                ))}
                            </select>
                            <div className="mt-4 flex justify-end gap-2">
                                <button
                                    type="button"
                                    onClick={() => setShowConfig(false)}
                                    className="cursor-pointer rounded bg-gray-400 px-2 py-1 text-sm text-white"
                                >
                                    Cancel
                                </button>
                                <button
                                    type="button"
                                    onClick={handleAddIndicator}
                                    className="cursor-pointer rounded bg-gray-600 px-2 py-1 text-sm text-white"
                                >
                                    Add
                                </button>
                            </div>
                        </div>
                    )}
                </fieldset>
                <div className="mt-14 flex justify-end gap-4">
                    <button
                        type="button"
                        onClick={onClose}
                        className={btnClass}
                    >
                        Cancel
                    </button>
                    <button type="submit" className={btnClass}>
                        Add Market
                    </button>
                </div>
            </form>
        </div>
    );
};
