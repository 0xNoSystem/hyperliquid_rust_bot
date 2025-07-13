import './index.css';
import MarketCard from './components/MarketCard'
import MarketsPage from './components/Markets'
import Header from './components/Header'
import type {IndicatorKind, MarketInfo} from './types'


const handleTogglePause = (asset: string) => {
  console.log(`Toggled pause for ${asset}`);
};

const handleRemove = (asset: string) => {
  console.log(`Removed market ${asset}`);
};

const App: React.FC = () => (
    <div className= "bg-[#1D1D1D] min-h-screen">
        <Header />
        <MarketsPage/>
    </div>
);

export default App;


