import './index.css';
import MarketsPage from './components/Markets';
import Layout from './components/Layout';
import Settings from "./components/Settings";
import { BrowserRouter, Routes, Route } from "react-router-dom";

const App: React.FC = () => (
  <BrowserRouter>
    <Routes>
      <Route path="/" element={<Layout />}>
        <Route index element={<MarketsPage />} />
        <Route path="settings" element={<Settings />} />
      </Route>
    </Routes>
  </BrowserRouter>
);

export default App;
