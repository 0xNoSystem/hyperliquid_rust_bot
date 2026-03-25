import { createContext, useContext } from "react";

export interface AuthState {
    token: string | null;
    address: string | null;
    isAuthenticated: boolean;
    isLoading: boolean;
    error: string | null;
    login: (token: string, address: string) => void;
    logout: () => void;
    clearError: () => void;
}

export const AuthContext = createContext<AuthState | undefined>(undefined);

export const useAuth = (): AuthState => {
    const ctx = useContext(AuthContext);
    if (!ctx) throw new Error("useAuth must be used within AuthProvider");
    return ctx;
};
