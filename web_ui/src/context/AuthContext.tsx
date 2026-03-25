import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { AuthContext } from "./AuthContextStore";

const TOKEN_KEY = "kwant-auth-token";
const ADDRESS_KEY = "kwant-auth-address";

function isTokenExpired(token: string): boolean {
    try {
        const payload = JSON.parse(atob(token.split(".")[1]));
        return payload.exp * 1000 < Date.now();
    } catch {
        return true;
    }
}

export function AuthProvider({ children }: { children: ReactNode }) {
    const [token, setToken] = useState<string | null>(null);
    const [address, setAddress] = useState<string | null>(null);
    const [isLoading, setIsLoading] = useState(true);
    const [error, setError] = useState<string | null>(null);

    // Hydrate from localStorage on mount
    useEffect(() => {
        try {
            const storedToken = localStorage.getItem(TOKEN_KEY);
            const storedAddress = localStorage.getItem(ADDRESS_KEY);
            if (storedToken && storedAddress && !isTokenExpired(storedToken)) {
                setToken(storedToken);
                setAddress(storedAddress);
            } else {
                localStorage.removeItem(TOKEN_KEY);
                localStorage.removeItem(ADDRESS_KEY);
            }
        } catch {
            // localStorage unavailable
        }
        setIsLoading(false);
    }, []);

    const login = useCallback((newToken: string, newAddress: string) => {
        setToken(newToken);
        setAddress(newAddress);
        setError(null);
        try {
            localStorage.setItem(TOKEN_KEY, newToken);
            localStorage.setItem(ADDRESS_KEY, newAddress);
        } catch {
            // localStorage unavailable
        }
    }, []);

    const logout = useCallback(() => {
        setToken(null);
        setAddress(null);
        try {
            localStorage.removeItem(TOKEN_KEY);
            localStorage.removeItem(ADDRESS_KEY);
        } catch {
            // localStorage unavailable
        }
    }, []);

    const clearError = useCallback(() => setError(null), []);

    const value = useMemo(
        () => ({
            token,
            address,
            isAuthenticated: token !== null && !isTokenExpired(token),
            isLoading,
            error,
            login,
            logout,
            clearError,
        }),
        [token, address, isLoading, error, login, logout, clearError]
    );

    return (
        <AuthContext.Provider value={value}>{children}</AuthContext.Provider>
    );
}
