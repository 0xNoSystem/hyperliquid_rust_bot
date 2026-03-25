// src/App.tsx
import "./index.css";
import MarketsPage from "./components/Markets";
import Layout from "./components/Layout";
import Backtest from "./components/Backtest";
import Settings from "./components/Settings";
import MarketDetail from "./components/MarketDetail";
import Login from "./components/Login";
import RequireAuth from "./components/RequireAuth";
import { BrowserRouter, Routes, Route } from "react-router-dom";
import ChartProvider from "./chart/ChartContext";

const App: React.FC = () => (
    <BrowserRouter>
        <Routes>
            <Route path="/login" element={<Login />} />
            <Route
                path="/"
                element={
                    <RequireAuth>
                        <Layout />
                    </RequireAuth>
                }
            >
                <Route index element={<MarketsPage />} />
                <Route path="asset/:asset" element={<MarketDetail />} />
                <Route path="settings" element={<Settings />} />
                <Route
                    path="backtest/:asset"
                    element={
                        <ChartProvider>
                            <Backtest />
                        </ChartProvider>
                    }
                />
            </Route>
        </Routes>
    </BrowserRouter>
);

export default App;
