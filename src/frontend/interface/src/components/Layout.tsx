import { Outlet } from "react-router-dom";
import Header from './Header'
import Footer from './footer'
import { BackgroundFX } from "../components/BackgroundFX";

export default function Layout() {
  return (
    <div className= "flex min-h-screen flex-col bg-[#1D1D1D] text-white">
        <BackgroundFX intensity={1} />
      <Header /> 
      <main className="flex-1">
        <Outlet /> 
      </main>
      <Footer />
    </div>
  );
}
