import { Outlet } from "react-router-dom";
import Header from "./Header";
import Footer from "./footer";
import { BackgroundFX } from "../components/BackgroundFX";

export default function Layout() {
    return (
        <div className="relative flex min-h-screen flex-col bg-[#1D1D1D] text-white">
            <BackgroundFX intensity={1} />
            <Header />
            <main className="flex flex-1 flex-col">
                <Outlet />
            </main>
            <Footer />
        </div>
    );
}
