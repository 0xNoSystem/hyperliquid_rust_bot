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
        <header className="border-line-subtle bg-app-surface-1 top-0 z-40 border-b py-4">
            <div className="mx-auto flex max-w-[2250px] items-center justify-between px-6 py-3">
                <Link to="/">
                    <div className="flex items-center gap-3">
                        <div className="border-line-subtle bg-app-surface-2 grid h-8 w-8 place-items-center rounded-md border">
                            <div className="bg-accent-brand-strong h-3.5 w-3.5" />
                        </div>
                        <div className="leading-tight">
                            <h1 className="text-app-text font-mono text-sm tracking-[0.18em]">
                                KWANT
                            </h1>
                            <p className="text-app-text/50 text-[10px] uppercase">
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
                        <div className="text-md border-accent-brand-strong/60 text-app-text relative right-20 w-fit rounded border px-3 py-1 font-semibold">
                            {"BACKTESTING (BETA)"}
                        </div>
                    </Link>
                    <a
                        href="https://app.hyperliquid.xyz"
                        target="_blank"
                        rel="noopener noreferrer"
                        className="border-line-subtle bg-app-surface-2 text-app-text hover:bg-glow-5 hidden items-center gap-2 rounded-md border px-3 py-1 text-[12px] md:inline-flex"
                    >
                        <ExternalLink className="text-accent-brand h-3.5 w-3.5" />{" "}
                        Hyperliquid
                    </a>
                    <a
                        href="https://github.com/0xNoSystem/hyperliquid_rust_bot"
                        target="_blank"
                        rel="noopener noreferrer"
                        className="border-line-subtle bg-app-surface-2 text-app-text hover:bg-glow-5 inline-flex items-center gap-2 rounded-md border px-3 py-1"
                    >
                        <Github className="h-4 w-4" />{" "}
                        <span className="text-[12px]">Repo</span>
                    </a>
                    <button
                        type="button"
                        onClick={toggleTheme}
                        className="border-line-subtle bg-app-surface-2 text-app-text hover:bg-glow-5 inline-flex items-center gap-2 rounded-md border px-3 py-1"
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
                        className="border-accent-brand bg-app-surface-2 text-app-text hover:bg-glow-5 inline-flex cursor-pointer items-center gap-2 rounded-md border px-3 py-1"
                    >
                        Settings
                    </Link>
                </div>
            </div>
        </header>
    );
};

export default Header;
