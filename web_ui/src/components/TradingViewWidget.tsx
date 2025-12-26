import { useEffect, useRef, memo } from "react";
import { useTheme } from "../context/ThemeContextStore";

type Props = {
    symbol: string; // e.g. "CRYPTO:BTCUSD"
    interval?: "1" | "5" | "15" | "60" | "240" | "D" | "W";
    theme?: "dark" | "light";
};

function TradingViewWidget({ symbol, interval = "D", theme }: Props) {
    const container = useRef<HTMLDivElement | null>(null);
    const { theme: appTheme } = useTheme();
    const resolvedTheme = theme ?? appTheme;
    if (symbol[0] == "k") {
        symbol = symbol.slice(1);
    }

    symbol = `CRYPTO:${symbol}USD`;

    useEffect(() => {
        if (!container.current) return;

        // clear previous embeds when symbol changes
        container.current.innerHTML = "";

        const rootStyle = getComputedStyle(document.documentElement);
        const tvBg = rootStyle.getPropertyValue("--tv-bg").trim() || "15 15 15";
        const tvGrid =
            rootStyle.getPropertyValue("--tv-grid").trim() || "242 242 242";
        const tvGridAlpha =
            rootStyle.getPropertyValue("--tv-grid-alpha").trim() || "0.06";

        const script = document.createElement("script");
        script.src =
            "https://s3.tradingview.com/external-embedding/embed-widget-advanced-chart.js";
        script.type = "text/javascript";
        script.async = true;

        script.innerHTML = JSON.stringify({
            allow_symbol_change: false,
            calendar: false,
            details: true,
            hide_side_toolbar: true,
            hide_top_toolbar: false,
            hide_legend: false,
            hide_volume: false,
            hotlist: false,
            interval,
            locale: "en",
            save_image: true,
            style: "1",
            symbol, // <- dynamic
            theme: resolvedTheme,
            timezone: "Etc/UTC",
            backgroundColor: `rgb(${tvBg})`,
            gridColor: `rgb(${tvGrid} / ${tvGridAlpha})`,
            withdateranges: false,
            autosize: true,
            studies: [],
            watchlist: [],
            compareSymbols: [],
        });

        container.current.appendChild(script);
    }, [symbol, interval, resolvedTheme]);

    return (
        <div ref={container} className="h-full w-full">
            <div className="tradingview-widget-container__widget h-full w-full" />
        </div>
    );
}

export default memo(TradingViewWidget);
