import React, { useState } from "react";
import { Github, ExternalLink, Moon, Sun, LogOut, Menu, X } from "lucide-react";
import { Link, useNavigate } from "react-router-dom";
import { useWebSocketContext } from "../context/WebSocketContextStore";
import { useTheme } from "../context/ThemeContextStore";
import { useAuth } from "../context/AuthContextStore";
import RotatingCube from "./Cube";

const Header: React.FC = () => {
    const { isOffline } = useWebSocketContext();
    const { theme, toggleTheme } = useTheme();
    const { address, logout } = useAuth();
    const navigate = useNavigate();
    const [menuOpen, setMenuOpen] = useState(false);

    const handleDisconnect = () => {
        logout();
        navigate("/login", { replace: true });
    };
    const isLight = theme === "light";

    return (
        <header className="border-line-subtle bg-app-surface-1/30 top-0 z-40 border-b py-2">
            <div className="mx-auto flex max-w-[2250px] items-center justify-between px-6 py-1">
                {/* Logo */}
                <Link to="/">
                    <div className="flex items-center gap-3">
                        <div className="border-line-subtle bg-app-surface-2 grid place-items-center rounded-md border">
                            <RotatingCube
                                foreground={isOffline ? "red" : "#05DF72"}
                            />
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

                {/* Desktop nav */}
                <div className="hidden md:flex items-center gap-2">
                    <Link to="/backtest/BTC">
                        <div className="hover:border-accent-brand-strong/60 text-app-text relative w-fit rounded border px-3 py-1 text-sm">
                            BACKTESTING
                        </div>
                    </Link>
                    <a
                        href="https://app.hyperliquid.xyz"
                        target="_blank"
                        rel="noopener noreferrer"
                        className="border-line-subtle bg-app-surface-2 text-app-text hover:bg-glow-5 inline-flex items-center gap-2 rounded-md border px-3 py-1 text-[12px]"
                    >
                        <ExternalLink className="text-accent-brand h-3.5 w-3.5" />
                        Hyperliquid
                    </a>
                    <a
                        href="https://github.com/0xNoSystem/hyperliquid_rust_bot"
                        target="_blank"
                        rel="noopener noreferrer"
                        className="border-line-subtle bg-app-surface-2 text-app-text hover:bg-glow-5 inline-flex items-center gap-2 rounded-md border px-3 py-1"
                    >
                        <Github className="h-4 w-4" />
                        <span className="text-[12px]">Repo</span>
                    </a>
                    <button
                        type="button"
                        onClick={toggleTheme}
                        className="border-line-subtle bg-app-surface-2 text-app-text hover:bg-glow-5 inline-flex items-center gap-2 rounded-md border px-3 py-1"
                        aria-label={`Switch to ${isLight ? "dark" : "light"} theme`}
                    >
                        {isLight ? <Moon className="h-4 w-4" /> : <Sun className="h-4 w-4" />}
                        <span className="text-[12px]">{isLight ? "Dark" : "Light"}</span>
                    </button>
                    <Link
                        to="/settings"
                        className="border-accent-brand bg-app-surface-2 text-app-text hover:bg-glow-5 inline-flex cursor-pointer items-center gap-2 rounded-md border px-3 py-1"
                    >
                        Settings
                    </Link>
                    <button
                        onClick={handleDisconnect}
                        className="border-line-subtle bg-app-surface-2 text-app-text hover:bg-accent-danger-soft/20 hover:text-accent-danger-soft inline-flex items-center gap-2 rounded-md border px-3 py-1"
                        title={address ? `${address.slice(0, 6)}…${address.slice(-4)}` : "Disconnect"}
                    >
                        <LogOut className="h-4 w-4" />
                        <span className="text-[12px]">
                            {address ? `${address.slice(0, 6)}…${address.slice(-4)}` : "Disconnect"}
                        </span>
                    </button>
                </div>

                {/* Mobile burger */}
                <button
                    type="button"
                    className="border-line-subtle bg-app-surface-2 text-app-text md:hidden inline-flex items-center justify-center rounded-md border p-2"
                    onClick={() => setMenuOpen((o) => !o)}
                    aria-label="Toggle menu"
                >
                    {menuOpen ? <X className="h-5 w-5" /> : <Menu className="h-5 w-5" />}
                </button>
            </div>

            {/* Mobile dropdown */}
            {menuOpen && (
                <div className="border-line-subtle bg-app-surface-1 md:hidden border-t px-6 py-3 flex flex-col gap-2">
                    <Link
                        to="/backtest/BTC"
                        onClick={() => setMenuOpen(false)}
                        className="hover:border-accent-brand-strong/60 text-app-text rounded border px-3 py-2 text-sm text-center"
                    >
                        Backtesting
                    </Link>
                    <a
                        href="https://app.hyperliquid.xyz"
                        target="_blank"
                        rel="noopener noreferrer"
                        className="border-line-subtle bg-app-surface-2 text-app-text hover:bg-glow-5 inline-flex items-center justify-center gap-2 rounded-md border px-3 py-2 text-[12px]"
                    >
                        <ExternalLink className="text-accent-brand h-3.5 w-3.5" />
                        Hyperliquid
                    </a>
                    <a
                        href="https://github.com/0xNoSystem/hyperliquid_rust_bot"
                        target="_blank"
                        rel="noopener noreferrer"
                        className="border-line-subtle bg-app-surface-2 text-app-text hover:bg-glow-5 inline-flex items-center justify-center gap-2 rounded-md border px-3 py-2"
                    >
                        <Github className="h-4 w-4" />
                        <span className="text-[12px]">Repo</span>
                    </a>
                    <button
                        type="button"
                        onClick={() => { toggleTheme(); setMenuOpen(false); }}
                        className="border-line-subtle bg-app-surface-2 text-app-text hover:bg-glow-5 inline-flex items-center justify-center gap-2 rounded-md border px-3 py-2"
                    >
                        {isLight ? <Moon className="h-4 w-4" /> : <Sun className="h-4 w-4" />}
                        <span className="text-[12px]">{isLight ? "Dark" : "Light"}</span>
                    </button>
                    <Link
                        to="/settings"
                        onClick={() => setMenuOpen(false)}
                        className="border-accent-brand bg-app-surface-2 text-app-text hover:bg-glow-5 inline-flex cursor-pointer items-center justify-center gap-2 rounded-md border px-3 py-2"
                    >
                        Settings
                    </Link>
                    <button
                        onClick={() => { setMenuOpen(false); handleDisconnect(); }}
                        className="border-line-subtle bg-app-surface-2 text-app-text hover:bg-accent-danger-soft/20 hover:text-accent-danger-soft inline-flex items-center justify-center gap-2 rounded-md border px-3 py-2"
                    >
                        <LogOut className="h-4 w-4" />
                        <span className="text-[12px]">
                            {address ? `${address.slice(0, 6)}…${address.slice(-4)}` : "Disconnect"}
                        </span>
                    </button>
                </div>
            )}
        </header>
    );
};

export default Header;
