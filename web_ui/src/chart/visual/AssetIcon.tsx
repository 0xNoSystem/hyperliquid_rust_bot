import { useEffect, useState } from "react";

type AssetIconProps = {
    symbol: string;
    size?: number;
    className?: string;
};

export default function AssetIcon({ symbol, size = 24, className }: AssetIconProps) {
    const sym = symbol.toLowerCase();
    const url = `https://raw.githubusercontent.com/spothq/cryptocurrency-icons/master/128/color/${sym}.png`;

    const [exists, setExists] = useState<boolean | null>(null);

    useEffect(() => {
        let cancelled = false;

        const check = async () => {
            try {
                const res = await fetch(url, { method: "HEAD" });

                if (!cancelled) {
                    setExists(res.ok); // true if 200, false if 404
                }
            } catch {
                if (!cancelled) setExists(false);
            }
        };

        check();

        return () => {
            cancelled = true;
        };
    }, [url]);

    // still checking
    if (exists === null) return null;

    // not found → render nothing (or a fallback icon)
    if (!exists) return null;

    return (
        <img
            src={url}
            alt={symbol}
            width={size}
            height={size}
            className={className}
        />
    );
}

