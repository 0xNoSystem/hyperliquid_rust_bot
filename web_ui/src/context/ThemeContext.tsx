import { useEffect, useMemo, useState, type ReactNode } from "react";
import { ThemeContext, type Theme } from "./ThemeContextStore";

const THEME_STORAGE_KEY = "kwant-theme";

const getInitialTheme = (): Theme => {
    if (typeof window === "undefined") {
        return "dark";
    }

    let stored: string | null = null;
    try {
        stored = window.localStorage.getItem(THEME_STORAGE_KEY);
    } catch {
        stored = null;
    }

    if (stored === "light" || stored === "dark") {
        document.documentElement.dataset.theme = stored;
        return stored;
    }

    const prefersLight = window.matchMedia?.(
        "(prefers-color-scheme: light)"
    ).matches;
    const next = prefersLight ? "light" : "dark";
    document.documentElement.dataset.theme = next;
    return next;
};

export function ThemeProvider({ children }: { children: ReactNode }) {
    const [theme, setTheme] = useState<Theme>(getInitialTheme);

    useEffect(() => {
        document.documentElement.dataset.theme = theme;
        try {
            window.localStorage.setItem(THEME_STORAGE_KEY, theme);
        } catch {
            // Ignore storage write errors (private mode, etc.)
        }
    }, [theme]);

    const value = useMemo(
        () => ({
            theme,
            setTheme,
            toggleTheme: () =>
                setTheme((prev) => (prev === "dark" ? "light" : "dark")),
        }),
        [theme]
    );

    return (
        <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>
    );
}
