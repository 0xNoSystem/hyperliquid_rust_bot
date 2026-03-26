import {
    useCallback,
    useEffect,
    useMemo,
    useState,
    type ReactNode,
} from "react";
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

    // Hydrate from localStorage on mount — verify wallet is still connected
    useEffect(() => {
        const hydrate = async () => {
            try {
                const storedToken = localStorage.getItem(TOKEN_KEY);
                const storedAddress = localStorage.getItem(ADDRESS_KEY);
                if (
                    !storedToken ||
                    !storedAddress ||
                    isTokenExpired(storedToken)
                ) {
                    localStorage.removeItem(TOKEN_KEY);
                    localStorage.removeItem(ADDRESS_KEY);
                    setIsLoading(false);
                    return;
                }

                // Check if wallet is still connected with the same address
                const phantom = (window as Record<string, unknown>).phantom as
                    | {
                          ethereum?: {
                              request: (a: {
                                  method: string;
                              }) => Promise<unknown>;
                          };
                      }
                    | undefined;
                const provider = phantom?.ethereum;
                if (provider) {
                    const accounts = (await provider.request({
                        method: "eth_accounts",
                    })) as string[];
                    const active = accounts[0]?.toLowerCase();
                    if (!active || active !== storedAddress.toLowerCase()) {
                        // Wallet disconnected or switched — discard stored session
                        localStorage.removeItem(TOKEN_KEY);
                        localStorage.removeItem(ADDRESS_KEY);
                        setIsLoading(false);
                        return;
                    }
                }

                setToken(storedToken);
                setAddress(storedAddress);
            } catch {
                localStorage.removeItem(TOKEN_KEY);
                localStorage.removeItem(ADDRESS_KEY);
            }
            setIsLoading(false);
        };
        hydrate();
    }, []);

    // Watch for wallet account changes — log out if active address no longer matches
    useEffect(() => {
        if (!address) return;

        const phantom = (window as Record<string, unknown>).phantom as
            | {
                  ethereum?: {
                      on: (e: string, h: (...a: unknown[]) => void) => void;
                      removeListener: (
                          e: string,
                          h: (...a: unknown[]) => void
                      ) => void;
                  };
              }
            | undefined;
        const provider = phantom?.ethereum;
        if (!provider) return;

        const handler = (accounts: unknown) => {
            const list = accounts as string[];
            const active = list[0]?.toLowerCase();
            if (!active || active !== address.toLowerCase()) {
                // Wallet switched or disconnected — force re-login
                setToken(null);
                setAddress(null);
                try {
                    localStorage.removeItem(TOKEN_KEY);
                    localStorage.removeItem(ADDRESS_KEY);
                } catch {
                    /* noop */
                }
            }
        };

        provider.on("accountsChanged", handler);
        return () => provider.removeListener("accountsChanged", handler);
    }, [address]);

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
