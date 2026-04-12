import { useState, useCallback, useRef, useEffect, useMemo } from "react";
import AceEditor from "react-ace";
import CustomRhaiMode from "../editor/custom_mode";
import "ace-builds/src-noconflict/theme-monokai";
import "ace-builds/src-noconflict/theme-solarized_light";
import "ace-builds/src-noconflict/keybinding-vim";

import { useTheme } from "../context/ThemeContextStore";

import { useLocation } from "react-router-dom";
import {
    Plus,
    Save,
    Trash2,
    ChevronRight,
    FlaskConical,
    Pencil,
    X,
    Maximize2,
} from "lucide-react";
import { motion, AnimatePresence } from "framer-motion";
import { useAuth } from "../context/AuthContextStore";
import { useWebSocketContext } from "../context/WebSocketContextStore";
import { API_URL } from "../consts";
import {
    into,
    TIMEFRAME_CAMELCASE,
    indicatorLabels,
    indicatorColors,
    indicatorParamLabels,
    indicatorKinds,
    get_params,
    fromTimeFrame,
} from "../types";
import type { IndexId, IndicatorKind, IndicatorName } from "../types";
import type { Strategy, StrategyDetail } from "../strats";
import SearchBar from "./SearchBar";

type TimeframeKey = keyof typeof TIMEFRAME_CAMELCASE;

/** Mirrors Rust `IndicatorKind::key()` + `_` + `TimeFrame::as_str()` */
function indicatorKey(asset: string, ind: IndicatorKind, tf: string): string {
    if ("rsi" in ind) return `${asset}_rsi_${ind.rsi}_${tf}`;
    if ("atr" in ind) return `${asset}_atr_${ind.atr}_${tf}`;
    if ("ema" in ind) return `${asset}_ema_${ind.ema}_${tf}`;
    if ("sma" in ind) return `${asset}_sma_${ind.sma}_${tf}`;
    if ("volMa" in ind) return `${asset}_volMa_${ind.volMa}_${tf}`;
    if ("histVolatility" in ind)
        return `${asset}_histVol_${ind.histVolatility}_${tf}`;
    if ("smaOnRsi" in ind)
        return `${asset}_smaRsi_${ind.smaOnRsi.periods}_${ind.smaOnRsi.smoothing_length}_${tf}`;
    if ("stochRsi" in ind)
        return `${asset}_stochRsi_${ind.stochRsi.periods}_${ind.stochRsi.k_smoothing ?? 0}_${ind.stochRsi.d_smoothing ?? 0}_${tf}`;
    if ("adx" in ind)
        return `${asset}_adx_${ind.adx.periods}_${ind.adx.di_length}_${tf}`;
    if ("emaCross" in ind)
        return `${asset}_emaCross_${ind.emaCross.short}_${ind.emaCross.long}_${tf}`;
    return "unknown";
}

const EMPTY_DETAIL: StrategyDetail = {
    id: "",
    name: "",
    onIdle: "",
    onOpen: "",
    onBusy: "",
    indicators: [],
    stateDeclarations: null,
};

/** Parse state declarations text (one per line: `name = value`) into a Record. */
function parseStateDeclarations(
    text: string
): Record<string, number | string | boolean | null> | null {
    const lines = text
        .split("\n")
        .map((l) => l.trim())
        .filter((l) => l.length > 0);
    if (lines.length === 0) return null;
    const result: Record<string, number | string | boolean | null> = {};
    for (const line of lines) {
        const match = line.match(/^(\w+)\s*=\s*(.+)$/);
        if (!match) continue;
        const [, name, rawVal] = match;
        const val = rawVal.trim();
        if (val === "null") result[name] = null;
        else if (val === "true") result[name] = true;
        else if (val === "false") result[name] = false;
        else if (/^".*"$/.test(val)) result[name] = val.slice(1, -1);
        else if (!isNaN(Number(val))) result[name] = Number(val);
        else result[name] = val;
    }
    return Object.keys(result).length > 0 ? result : null;
}

/** Serialize state declarations Record back to editable text. */
function serializeStateDeclarations(
    decls: Record<string, number | string | boolean | null> | null | undefined
): string {
    if (!decls) return "";
    return Object.entries(decls)
        .map(([k, v]) => {
            if (v === null) return `${k} = null`;
            if (typeof v === "string") return `${k} = "${v}"`;
            return `${k} = ${v}`;
        })
        .join("\n");
}

export default function StratEditor() {
    const { token } = useAuth();
    const { strategies, fetchStrategies, universe } = useWebSocketContext();
    const location = useLocation();
    const { theme } = useTheme();

    const rhaiMode = useMemo(() => new CustomRhaiMode(), []);

    // Currently selected / editing strategy
    const [active, setActive] = useState<StrategyDetail | null>(null);
    const [isNew, setIsNew] = useState(false);
    const [editing, setEditing] = useState(false);
    const [loading, setLoading] = useState(false);
    const [saving, setSaving] = useState(false);
    const [error, setError] = useState<string | null>(null);
    const [success, setSuccess] = useState<string | null>(null);

    // Editable fields
    const [name, setName] = useState("");
    const [onIdle, setOnIdle] = useState("");
    const [onOpen, setOnOpen] = useState("");
    const [onBusy, setOnBusy] = useState("");
    const [indicators, setIndicators] = useState<IndexId[]>([]);
    const [stateText, setStateText] = useState("");

    //vim
    const [vimMode, setVimMode] = useState<boolean>(false);

    // Fullscreen editor
    const [fullscreenPanel, setFullscreenPanel] = useState<string | null>(null);
    const [hoveredPanel, setHoveredPanel] = useState<string | null>(null);

    useEffect(() => {
        if (!fullscreenPanel || vimMode) return;
        const handler = (e: KeyboardEvent) => {
            if (e.key === "Escape") setFullscreenPanel(null);
        };
        window.addEventListener("keydown", handler);
        return () => window.removeEventListener("keydown", handler);
    }, [fullscreenPanel, vimMode]);

    // Textarea refs for insert-at-cursor
    const textareaRefs = useRef<Record<string, AceEditor | null>>({});
    const lastFocusedRef = useRef<string | null>(null);

    const insertAtCursor = (text: string) => {
        const key = lastFocusedRef.current;
        if (!key) return;

        // The ref points to the AceEditor component instance
        const aceComponent = textareaRefs.current[key];
        if (!aceComponent) return;

        // Get the underlying Ace editor instance
        const editor = aceComponent.editor;

        // Use Ace's internal API to insert at cursor
        editor.insert(text);
        editor.focus();
    };
    // Indicator picker
    const [showIndicatorPicker, setShowIndicatorPicker] = useState(false);
    const [newAsset, setNewAsset] = useState("BTC");
    const [newKind, setNewKind] = useState<IndicatorName>("rsi");
    const [newParam, setNewParam] = useState(14);
    const [newParam2, setNewParam2] = useState(14);
    const [newTf, setNewTf] = useState<TimeframeKey>("15m");
    const assetOptions = useMemo(
        () =>
            [
                { value: "self", label: "self" },
                ...universe.map((asset) => ({
                    value: asset.name,
                    label: asset.name,
                })),
            ],
        [universe]
    );

    const loadStrategy = useCallback(
        async (strat: Strategy) => {
            setLoading(true);
            setError(null);
            try {
                const res = await fetch(`${API_URL}/strategies/${strat.id}`, {
                    headers: { Authorization: `Bearer ${token}` },
                });
                if (!res.ok) throw new Error("Failed to load strategy");
                const detail: StrategyDetail = await res.json();
                setActive(detail);
                setIsNew(false);
                setEditing(false);
                setName(detail.name);
                setOnIdle(detail.onIdle);
                setOnOpen(detail.onOpen);
                setOnBusy(detail.onBusy);
                setIndicators(detail.indicators);
                setStateText(
                    serializeStateDeclarations(detail.stateDeclarations)
                );
            } catch (e) {
                setError(
                    e instanceof Error ? e.message : "Failed to load strategy"
                );
            } finally {
                setLoading(false);
            }
        },
        [token]
    );

    // Auto-load strategy from navigation state (e.g. "Open in Lab" from MarketDetail)
    useEffect(() => {
        const state = location.state as { strategyId?: string } | null;
        if (!state?.strategyId || !strategies.length) return;
        const strat = strategies.find((s) => s.id === state.strategyId);
        if (strat && active?.id !== strat.id) {
            loadStrategy(strat);
            // Clear state so refresh doesn't re-trigger
            window.history.replaceState({}, "");
        }
    }, [location.state, strategies, loadStrategy, active?.id]);

    const startNew = () => {
        setActive(EMPTY_DETAIL);
        setIsNew(true);
        setEditing(true);
        setName("");
        setOnIdle("");
        setOnOpen("");
        setOnBusy("");
        setIndicators([]);
        setStateText("");
        setError(null);
        setSuccess(null);
    };

    const handleSave = async () => {
        if (!name.trim()) {
            setError("Strategy name is required");
            return;
        }
        setSaving(true);
        setError(null);
        setSuccess(null);
        try {
            const body = {
                name: name.trim(),
                on_idle: onIdle,
                on_open: onOpen,
                on_busy: onBusy,
                indicators: indicators,
                state_declarations: parseStateDeclarations(stateText),
                is_active: active?.isActive ?? false,
            };

            const url = isNew
                ? `${API_URL}/strategies`
                : `${API_URL}/strategies/${active!.id}`;
            const method = isNew ? "POST" : "PUT";

            const res = await fetch(url, {
                method,
                headers: {
                    "Content-Type": "application/json",
                    Authorization: `Bearer ${token}`,
                },
                body: JSON.stringify(body),
            });

            if (!res.ok) {
                const data = await res.json().catch(() => null);
                throw new Error(data?.error ?? `Save failed (${res.status})`);
            }

            const saved: StrategyDetail = await res.json();
            setActive(saved);
            setIsNew(false);
            setEditing(false);
            setName(saved.name);
            setOnIdle(saved.onIdle);
            setOnOpen(saved.onOpen);
            setOnBusy(saved.onBusy);
            setIndicators(saved.indicators);
            setStateText(serializeStateDeclarations(saved.stateDeclarations));
            setSuccess(isNew ? "Strategy created" : "Strategy saved");
            fetchStrategies();
            setTimeout(() => setSuccess(null), 3000);
        } catch (e) {
            setError(e instanceof Error ? e.message : "Save failed");
        } finally {
            setSaving(false);
        }
    };

    const handleDelete = async () => {
        if (!active || isNew) return;
        if (!window.confirm(`Delete "${active.name}"?`)) return;
        try {
            const res = await fetch(`${API_URL}/strategies/${active.id}`, {
                method: "DELETE",
                headers: { Authorization: `Bearer ${token}` },
            });
            if (!res.ok) throw new Error("Delete failed");
            setActive(null);
            setIsNew(false);
            fetchStrategies();
        } catch (e) {
            setError(e instanceof Error ? e.message : "Delete failed");
        }
    };

    // Indicator picker logic
    const handleAddIndicator = () => {
        let cfg: IndicatorKind;
        switch (newKind) {
            case "histVolatility":
                cfg = { histVolatility: newParam };
                break;
            case "volMa":
                cfg = { volMa: newParam };
                break;
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
            case "rsi":
                cfg = { rsi: newParam };
                break;
            case "atr":
                cfg = { atr: newParam };
                break;
            case "ema":
                cfg = { ema: newParam };
                break;
            case "sma":
                cfg = { sma: newParam };
                break;
            default:
                cfg = { rsi: newParam };
        }

        const newItem: IndexId = [newAsset, cfg, into(newTf)];
        setIndicators((prev) => {
            const exists = prev.some(
                (item) => JSON.stringify(item) === JSON.stringify(newItem)
            );
            return exists ? prev : [...prev, newItem];
        });
        setShowIndicatorPicker(false);
    };

    const removeIndicator = (i: number) =>
        setIndicators((prev) => prev.filter((_, idx) => idx !== i));

    const inputClass =
        "w-full rounded border border-line-solid bg-surface-input px-3 py-2 text-app-text text-sm font-mono";
    const selectClass =
        "w-full cursor-pointer rounded border border-line-solid bg-surface-input px-3 py-2 text-app-text text-sm";
    const hasDualParams = ["emaCross", "smaOnRsi", "adx"].includes(newKind);

    return (
        <div className="text-app-text z-1 flex h-[120vh] min-h-screen">
            {/* ---- Left sidebar: strategy list ---- */}
            <div className="border-line-subtle bg-surface-pane flex w-64 shrink-0 flex-col border-r">
                <div className="border-line-subtle flex items-center gap-2 border-b px-4 py-3">
                    <FlaskConical className="text-accent-brand-soft h-5 w-5" />
                    <h2 className="text-base font-semibold">Strategy Lab</h2>
                </div>

                <div className="flex-1 overflow-y-auto">
                    {strategies.map((s) => (
                        <button
                            key={s.id}
                            onClick={() => loadStrategy(s)}
                            className={`border-line-subtle flex w-full items-center justify-between border-b px-4 py-3 text-left text-sm transition ${
                                active?.id === s.id && !isNew
                                    ? "bg-glow-10 text-accent-brand-soft"
                                    : "text-app-text/70 hover:bg-glow-5"
                            }`}
                        >
                            <span className="truncate">{s.name}</span>
                            <ChevronRight className="h-4 w-4 shrink-0 opacity-40" />
                        </button>
                    ))}
                </div>

                <button
                    onClick={startNew}
                    className="border-line-subtle text-accent-brand-soft hover:bg-glow-5 flex items-center gap-2 border-t px-4 py-3 text-sm font-medium"
                >
                    <Plus className="h-4 w-4" />
                    New Strategy
                </button>
            </div>

            {/* ---- Main editor area ---- */}
            <div className="flex flex-1 flex-col overflow-hidden">
                {!active ? (
                    <div className="text-app-text/40 text-md flex flex-1 flex-col items-center justify-center">
                        Select a strategy or create a new one
                        <button
                            onClick={startNew}
                            className="border-line-subtle text-accent-brand-soft hover:bg-glow-5 mt-1 flex items-center gap-2 rounded-sm px-2 py-2 text-sm font-medium hover:bg-orange-200/30"
                        >
                            <Plus className="h-4 w-4" />
                            New Strategy
                        </button>
                    </div>
                ) : loading ? (
                    <div className="text-app-text/40 flex flex-1 items-center justify-center text-sm">
                        Loading...
                    </div>
                ) : (
                    <>
                        {/* Top bar: name + actions */}
                        <div className="border-line-subtle flex items-center gap-3 border-b px-6 py-3">
                            {editing ? (
                                <input
                                    type="text"
                                    value={name}
                                    onChange={(e) => setName(e.target.value)}
                                    placeholder="Strategy name"
                                    spellCheck={false}
                                    className="text-app-text placeholder:text-app-text/30 flex-1 bg-transparent text-lg font-semibold outline-none"
                                />
                            ) : (
                                <span className="text-app-text flex-1 text-lg font-semibold">
                                    {name || "Untitled"}
                                </span>
                            )}

                            {editing ? (
                                <>
                                    <button
                                        onClick={handleSave}
                                        disabled={saving}
                                        className="border-action-add-border bg-action-add-bg text-action-add-text hover:bg-action-add-hover flex items-center gap-1.5 rounded-md border px-3 py-1.5 text-sm disabled:opacity-50"
                                    >
                                        <Save className="h-3.5 w-3.5" />
                                        {saving ? "Saving..." : "Save"}
                                    </button>
                                    {!isNew && (
                                        <button
                                            onClick={handleDelete}
                                            className="border-accent-danger-soft/40 text-accent-danger-soft hover:bg-accent-danger-soft/10 flex items-center gap-1.5 rounded-md border px-3 py-1.5 text-sm"
                                        >
                                            <Trash2 className="h-3.5 w-3.5" />
                                            Delete
                                        </button>
                                    )}
                                    <button className="rounded bg-green-600/30 px-3 font-bold">
                                        <span
                                            className="font-normal"
                                            onClick={() => setVimMode(!vimMode)}
                                        >
                                            vim {vimMode ? "on " : "Off "}
                                        </span>
                                    </button>
                                </>
                            ) : (
                                <button
                                    onClick={() => setEditing(true)}
                                    className="border-line-subtle text-accent-brand-soft hover:bg-glow-5 flex items-center gap-1.5 rounded-md border px-3 py-1.5 text-sm"
                                >
                                    <Pencil className="h-3.5 w-3.5" />
                                    Edit
                                </button>
                            )}
                        </div>

                        {/* Status messages */}
                        <AnimatePresence>
                            {(error || success) && (
                                <motion.div
                                    initial={{ height: 0, opacity: 0 }}
                                    animate={{ height: "auto", opacity: 1 }}
                                    exit={{ height: 0, opacity: 0 }}
                                    className="overflow-hidden"
                                >
                                    <div
                                        className={`px-6 py-2 text-sm ${
                                            error
                                                ? "bg-surface-pane text-accent-danger-soft"
                                                : "bg-surface-success text-success-faint"
                                        }`}
                                    >
                                        {error ?? success}
                                    </div>
                                </motion.div>
                            )}
                        </AnimatePresence>

                        {/* Indicators */}
                        <div className="border-line-subtle relative border-b px-6 py-3">
                            <div className="mb-2 flex items-center justify-between">
                                <span className="text-app-text/50 text-xs font-medium tracking-wide uppercase">
                                    Indicators
                                </span>
                                {editing && (
                                    <button
                                        onClick={() =>
                                            setShowIndicatorPicker((p) => !p)
                                        }
                                        className="text-accent-brand-soft text-xs hover:underline"
                                    >
                                        + Add
                                    </button>
                                )}
                            </div>
                            <div className="flex flex-wrap gap-2">
                                {indicators.length === 0 && (
                                    <span className="text-app-text/30 text-xs">
                                        No indicators added
                                    </span>
                                )}
                                {indicators.map(([asset, ind, tf], i) => {
                                    const kind = Object.keys(
                                        ind
                                    )[0] as IndicatorName;
                                    return (
                                        <div
                                            key={i}
                                            className="flex items-center gap-1"
                                        >
                                            <button
                                                type="button"
                                                onClick={() =>
                                                    insertAtCursor(
                                                        `let x = extract("${indicatorKey(asset, ind, fromTimeFrame(tf))}");`
                                                    )
                                                }
                                                title="Click to insert extract() into editor"
                                                className={`${indicatorColors[kind]} cursor-pointer rounded-full px-2.5 py-0.5 text-xs hover:opacity-80`}
                                            >
                                                <strong>({asset})</strong>{" "}
                                                {indicatorLabels[kind]}{" "}
                                                {get_params(ind)}{" "}
                                                {fromTimeFrame(tf)}
                                            </button>
                                            {editing && (
                                                <button
                                                    onClick={() =>
                                                        removeIndicator(i)
                                                    }
                                                    className="text-accent-danger-strong cursor-pointer text-sm leading-none"
                                                >
                                                    x
                                                </button>
                                            )}
                                        </div>
                                    );
                                })}
                            </div>

                            {/* Indicator picker popover */}
                            <AnimatePresence>
                                {showIndicatorPicker && (
                                    <motion.div
                                        initial={{ opacity: 0, y: -4 }}
                                        animate={{ opacity: 1, y: 0 }}
                                        exit={{ opacity: 0, y: -4 }}
                                        className="border-line-solid bg-surface-popover absolute top-full right-6 z-20 mt-1 w-72 rounded-md border p-4 shadow-lg"
                                    >
                                        <div className="mb-3 flex items-center justify-between">
                                            <h3 className="text-sm font-semibold">
                                                Add Indicator
                                            </h3>
                                            <button
                                                onClick={() =>
                                                    setShowIndicatorPicker(
                                                        false
                                                    )
                                                }
                                            >
                                                <X className="text-app-text/40 h-4 w-4" />
                                            </button>
                                        </div>
                                        <div className="mb-2">
                                            <label className="text-app-text/60 text-xs">
                                                Asset
                                            </label>
                                            <SearchBar
                                                value={newAsset}
                                                onChange={setNewAsset}
                                                options={assetOptions}
                                                placeholder="Select asset"
                                                searchPlaceholder="Search assets..."
                                                emptyMessage="No assets found."
                                                ariaLabel="Indicator asset"
                                                containerClassName="mt-1"
                                                buttonClassName={selectClass}
                                            />
                                        </div>
                                        <select
                                            value={newKind}
                                            onChange={(e) =>
                                                setNewKind(
                                                    e.target
                                                        .value as IndicatorName
                                                )
                                            }
                                            className={selectClass}
                                        >
                                            {indicatorKinds.map((k) => (
                                                <option key={k} value={k}>
                                                    {indicatorLabels[k]}
                                                </option>
                                            ))}
                                        </select>
                                        <div className="mt-2 grid grid-cols-2 gap-2">
                                            <label className="text-app-text/60 mt-1 text-right text-xs">
                                                {
                                                    indicatorParamLabels[
                                                        newKind
                                                    ][0]
                                                }
                                            </label>
                                            <input
                                                type="number"
                                                value={newParam}
                                                onChange={(e) =>
                                                    setNewParam(+e.target.value)
                                                }
                                                className={inputClass}
                                            />
                                            {hasDualParams && (
                                                <>
                                                    <label className="text-app-text/60 mt-1 text-right text-xs">
                                                        {
                                                            indicatorParamLabels[
                                                                newKind
                                                            ][1]
                                                        }
                                                    </label>
                                                    <input
                                                        type="number"
                                                        value={newParam2}
                                                        onChange={(e) =>
                                                            setNewParam2(
                                                                +e.target.value
                                                            )
                                                        }
                                                        className={inputClass}
                                                    />
                                                </>
                                            )}
                                        </div>
                                        <div className="mt-2">
                                            <label className="text-app-text/60 text-xs">
                                                Time Frame
                                            </label>
                                            <select
                                                value={newTf}
                                                onChange={(e) =>
                                                    setNewTf(
                                                        e.target
                                                            .value as TimeframeKey
                                                    )
                                                }
                                                className={selectClass}
                                            >
                                                {Object.keys(
                                                    TIMEFRAME_CAMELCASE
                                                ).map((t) => (
                                                    <option key={t} value={t}>
                                                        {t}
                                                    </option>
                                                ))}
                                            </select>
                                        </div>
                                        <button
                                            onClick={handleAddIndicator}
                                            className="border-action-add-border bg-action-add-bg text-action-add-text hover:bg-action-add-hover mt-3 w-full rounded-md border py-1.5 text-sm"
                                        >
                                            Add
                                        </button>
                                    </motion.div>
                                )}
                            </AnimatePresence>
                        </div>

                        {/* State declarations */}
                        {(editing || stateText.trim()) && (
                            <div className="border-line-subtle border-b px-6 py-3">
                                <div className="mb-2">
                                    <span className="text-app-text/50 text-xs font-medium tracking-wide uppercase">
                                        State Variables
                                    </span>
                                    <span className="text-app-text/30 ml-2 text-xs">
                                        one per line: name = default
                                    </span>
                                </div>
                                <textarea
                                    value={stateText}
                                    onChange={(e) =>
                                        setStateText(e.target.value)
                                    }
                                    readOnly={!editing}
                                    placeholder={
                                        'count = 0\nlast_signal = "none"'
                                    }
                                    rows={Math.max(
                                        2,
                                        stateText.split("\n").length
                                    )}
                                    className="border-line-solid bg-surface-input text-app-text placeholder:text-app-text/20 w-full resize-none rounded border px-3 py-2 font-mono text-sm"
                                />
                            </div>
                        )}

                        {/* Script editors */}
                        <div className="relative flex flex-1 flex-col gap-px overflow-hidden lg:flex-row">
                            {(
                                [
                                    ["on_idle", onIdle, setOnIdle],
                                    ["on_open", onOpen, setOnOpen],
                                    ["on_busy", onBusy, setOnBusy],
                                ] as const
                            ).map(([label, value, setter]) => {
                                const isFullscreen = fullscreenPanel === label;
                                return (
                                    <div
                                        key={label}
                                        className={
                                            isFullscreen
                                                ? "bg-app-surface-3 absolute inset-0 z-30 flex flex-col"
                                                : "bg-app-surface-3 relative flex flex-1 flex-col"
                                        }
                                        onMouseEnter={() =>
                                            setHoveredPanel(label)
                                        }
                                        onMouseLeave={() =>
                                            setHoveredPanel(null)
                                        }
                                    >
                                        <div className="border-line-subtle bg-surface-pane flex items-center justify-between border-b px-4 py-2">
                                            <span className="text-app-text/50 text-xs font-medium tracking-wider uppercase">
                                                {label.replace("_", " ")}
                                            </span>
                                            {isFullscreen && (
                                                <button
                                                    onClick={() =>
                                                        setFullscreenPanel(null)
                                                    }
                                                    className="text-app-text/40 hover:text-app-text transition"
                                                >
                                                    <X className="h-4 w-4" />
                                                </button>
                                            )}
                                        </div>
                                        {/* Fullscreen button on hover */}
                                        {!isFullscreen &&
                                            hoveredPanel === label && (
                                                <button
                                                    onClick={() =>
                                                        setFullscreenPanel(
                                                            label
                                                        )
                                                    }
                                                    className="absolute top-10 right-2 z-10 rounded bg-black/50 p-1.5 text-white/70 transition hover:text-white"
                                                    title="Fullscreen"
                                                >
                                                    <Maximize2 className="h-4 w-4" />
                                                </button>
                                            )}
                                        <AceEditor
                                            mode={rhaiMode}
                                            theme={
                                                theme == "light"
                                                    ? "solarized_light"
                                                    : "monokai"
                                            }
                                            ref={(el) => {
                                                textareaRefs.current[label] =
                                                    el;
                                            }}
                                            onFocus={() => {
                                                lastFocusedRef.current = label;
                                            }}
                                            value={value}
                                            onChange={(newValue) =>
                                                setter(newValue)
                                            }
                                            readOnly={!editing}
                                            setOptions={{
                                                useWorker: false,
                                                fontFamily: "monospace",
                                            }}
                                            className="flex-1"
                                            width="100%"
                                            height="100%"
                                            keyboardHandler={
                                                vimMode ? "vim" : undefined
                                            }
                                        />
                                    </div>
                                );
                            })}
                        </div>
                    </>
                )}
            </div>
        </div>
    );
}
