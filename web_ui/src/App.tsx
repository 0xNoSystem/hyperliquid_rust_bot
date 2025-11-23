// src/App.tsx
import "./index.css";
import MarketsPage from "./components/Markets";
import Layout from "./components/Layout";
import Backtest from "./components/Backtest";
import Settings from "./components/Settings";
import MarketDetail from "./components/MarketDetail";
import { BrowserRouter, Routes, Route } from "react-router-dom";

const App: React.FC = () => (
    <BrowserRouter>
        <Routes>
            <Route path="/" element={<Layout />}>
                <Route index element={<MarketsPage />} />
                <Route path="asset/:asset" element={<MarketDetail />} />
                <Route path="settings" element={<Settings />} />
                <Route path="backtest/:asset" element={<Backtest />} />
            </Route>
        </Routes>
    </BrowserRouter>
);

export default App;
