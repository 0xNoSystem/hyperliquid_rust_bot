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
        <header className="top-0 z-40 border-b border-line-subtle bg-app-surface-1 py-4">
            <div className="mx-auto flex max-w-[2250px] items-center justify-between px-6 py-3">
                <Link to="/">
                    <div className="flex items-center gap-3">
                        <div className="grid h-8 w-8 place-items-center rounded-md border border-line-subtle bg-app-surface-2">
                            <div className="h-3.5 w-3.5 bg-accent-brand-strong" />
                        </div>
                        <div className="leading-tight">
                            <h1 className="font-mono text-sm tracking-[0.18em] text-app-text">
                                KWANT
                            </h1>
                            <p className="text-[10px] text-app-text/50 uppercase">
                                Terminal{" "}
                                <span
                                    className={
                                        isOffline
                                            ? "text-accent-danger-soft"
                                            : "text-accent-success"
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
                        <div className="text-md relative right-20 w-fit rounded border border-accent-brand-strong/60 px-3 py-1 font-semibold text-app-text">
                            {"BACKTESTING (BETA)"}
                        </div>
                    </Link>
                    <a
                        href="https://app.hyperliquid.xyz"
                        target="_blank"
                        rel="noopener noreferrer"
                        className="hidden items-center gap-2 rounded-md border border-line-subtle bg-app-surface-2 px-3 py-1 text-[12px] text-app-text hover:bg-glow-5 md:inline-flex"
                    >
                        <ExternalLink className="h-3.5 w-3.5 text-accent-brand" />{" "}
                        Hyperliquid
                    </a>
                    <a
                        href="https://github.com/0xNoSystem/hyperliquid_rust_bot"
                        target="_blank"
                        rel="noopener noreferrer"
                        className="inline-flex items-center gap-2 rounded-md border border-line-subtle bg-app-surface-2 px-3 py-1 text-app-text hover:bg-glow-5"
                    >
                        <Github className="h-4 w-4" />{" "}
                        <span className="text-[12px]">Repo</span>
                    </a>
                    <button
                        type="button"
                        onClick={toggleTheme}
                        className="inline-flex items-center gap-2 rounded-md border border-line-subtle bg-app-surface-2 px-3 py-1 text-app-text hover:bg-glow-5"
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
                        className="inline-flex cursor-pointer items-center gap-2 rounded-md border border-accent-brand bg-app-surface-2 px-3 py-1 text-app-text hover:bg-glow-5"
                    >
                        Settings
                    </Link>
                </div>
            </div>
        </header>
    );
};

export default Header;
