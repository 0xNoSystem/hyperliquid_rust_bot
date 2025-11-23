import React, { useEffect, useRef, memo } from "react";

type Props = {
    symbol: string; // e.g. "CRYPTO:BTCUSD"
    interval?: "1" | "5" | "15" | "60" | "240" | "D" | "W";
    theme?: "dark" | "light";
};

function TradingViewWidget({ symbol, interval = "D", theme = "dark" }: Props) {
    const container = useRef<HTMLDivElement | null>(null);

    useEffect(() => {
        if (!container.current) return;

        // clear previous embeds when symbol changes
        container.current.innerHTML = "";

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
            theme,
            timezone: "Etc/UTC",
            backgroundColor: "#0F0F0F",
            gridColor: "rgba(242,242,242,0.06)",
            withdateranges: false,
            autosize: true,
            studies: [],
            watchlist: [],
            compareSymbols: [],
        });

        container.current.appendChild(script);
    }, [symbol, interval, theme]);

    return (
        <div ref={container} className="h-full w-full">
            <div className="tradingview-widget-container__widget h-full w-full" />
        </div>
    );
}

export default memo(TradingViewWidget);
