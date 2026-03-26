import React from "react";

interface CachedMarketProps {
    asset: string;
    onAdd: (asset: string) => void;
    onRemove: (asset: string) => void;
}

export const CachedMarket: React.FC<CachedMarketProps> = ({
    asset,
    onAdd,
    onRemove,
}) => {
    return (
        <div className="border-line-ink-strong bg-surface-input-soft/80 text-ink hover:bg-surface-input-soft my-2 flex items-center justify-between rounded-lg border px-3 py-2 text-sm font-medium">
            <span className="bg-ink-50 text-app-text/80 rounded-lg px-3 py-1 text-center">
                {asset}
            </span>
            <div className="flex gap-1">
                <button
                    className="hover:bg-ink-50 hover:text-accent-brand cursor-pointer rounded-lg px-3 py-1"
                    onClick={() => onAdd(asset)}
                >
                    Add
                </button>
                <button
                    className="hover:bg-ink-50 text-app-text/40 hover:text-accent-danger-strong cursor-pointer rounded-lg px-2 py-1"
                    onClick={() => onRemove(asset)}
                >
                    ×
                </button>
            </div>
        </div>
    );
};
