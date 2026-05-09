// src/App.tsx
import "./index.css";
import MarketsPage from "./components/Markets";
import Layout from "./components/Layout";
import Backtest from "./components/Backtest";
import BacktestRunDetail from "./components/BacktestRunDetail";
import Settings from "./components/Settings";
import MarketDetail from "./components/MarketDetail";
import StratEditor from "./components/StratEditor";
import Login from "./components/Login";
import RequireAuth from "./components/RequireAuth";
import Docs from "./components/Docs";
import { BrowserRouter, Routes, Route } from "react-router-dom";
import { WebSocketProvider } from "./context/WebSocketContext";
import ChartProvider from "./chart/ChartContext";

const App: React.FC = () => (
    <BrowserRouter>
        <Routes>
            <Route path="/login" element={<Login />} />
            <Route
                path="/"
                element={
                    <RequireAuth>
                        <WebSocketProvider>
                            <Layout />
                        </WebSocketProvider>
                    </RequireAuth>
                }
            >
                <Route index element={<MarketsPage />} />
                <Route path="asset/:asset" element={<MarketDetail />} />
                <Route path="settings" element={<Settings />} />
                <Route path="lab" element={<StratEditor />} />
                <Route path="docs" element={<Docs />} />
                <Route
                    path="backtest/:asset"
                    element={
                        <ChartProvider>
                            <Backtest />
                        </ChartProvider>
                    }
                />
                <Route
                    path="backtest/run/:runId"
                    element={<BacktestRunDetail />}
                />
            </Route>
        </Routes>
    </BrowserRouter>
);

export default App;
