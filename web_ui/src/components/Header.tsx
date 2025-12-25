import React from "react";
import { Github, ExternalLink, Moon, Sun } from "lucide-react";
import { Link } from "react-router-dom";
import { useWebSocketContext } from "../context/WebSocketContextStore";
import { useTheme } from "../context/ThemeContextStore";

const Header: React.FC = () => {
    const { isOffline } = useWebSocketContext();
    const { theme, toggleTheme } = useTheme();
    const isLight = theme === "light";
    return (
        <header className="top-0 z-40 border-b border-white/10 bg-[#0B0C0E] py-4">
            <div className="mx-auto flex max-w-[2250px] items-center justify-between px-6 py-3">
                <Link to="/">
                    <div className="flex items-center gap-3">
                        <div className="grid h-8 w-8 place-items-center rounded-md border border-white/10 bg-[#111316]">
                            <div className="h-3.5 w-3.5 bg-orange-500" />
                        </div>
                        <div className="leading-tight">
                            <h1 className="font-mono text-sm tracking-[0.18em] text-white">
                                KWANT
                            </h1>
                            <p className="text-[10px] text-white/50 uppercase">
                                Terminal{" "}
                                <span
                                    className={
                                        isOffline
                                            ? "text-red-400"
                                            : "text-green-400"
                                    }
                                >
                                    {isOffline ? "Offline" : "Online"}
                                </span>
                            </p>
                        </div>
                    </div>
                </Link>

                <div className="flex items-center gap-2">
                    <Link to="/backtest/BTC">
                        <div className="text-md relative right-20 w-fit rounded border border-orange-500/60 px-3 py-1 font-semibold text-white">
                            {"BACKTESTING (BETA)"}
                        </div>
                    </Link>
                    <a
                        href="https://app.hyperliquid.xyz"
                        target="_blank"
                        rel="noopener noreferrer"
                        className="hidden items-center gap-2 rounded-md border border-white/10 bg-[#111316] px-3 py-1 text-[12px] text-white hover:bg-white/5 md:inline-flex"
                    >
                        <ExternalLink className="h-3.5 w-3.5 text-orange-400" />{" "}
                        Hyperliquid
                    </a>
                    <a
                        href="https://github.com/0xNoSystem/hyperliquid_rust_bot"
                        target="_blank"
                        rel="noopener noreferrer"
                        className="inline-flex items-center gap-2 rounded-md border border-white/10 bg-[#111316] px-3 py-1 text-white hover:bg-white/5"
                    >
                        <Github className="h-4 w-4" />{" "}
                        <span className="text-[12px]">Repo</span>
                    </a>
                    <button
                        type="button"
                        onClick={toggleTheme}
                        className="inline-flex items-center gap-2 rounded-md border border-white/10 bg-[#111316] px-3 py-1 text-white hover:bg-white/5"
                        aria-label={`Switch to ${
                            isLight ? "dark" : "light"
                        } theme`}
                    >
                        {isLight ? (
                            <Moon className="h-4 w-4" />
                        ) : (
                            <Sun className="h-4 w-4" />
                        )}
                        <span className="text-[12px]">
                            {isLight ? "Dark" : "Light"}
                        </span>
                    </button>

                    <Link
                        to="/settings"
                        className="inline-flex cursor-pointer items-center gap-2 rounded-md border border-orange-400 bg-[#111316] px-3 py-1 text-white hover:bg-white/5"
                    >
                        Settings
                    </Link>
                </div>
            </div>
        </header>
    );
};

export default Header;
