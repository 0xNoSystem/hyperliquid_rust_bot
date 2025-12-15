import { Outlet } from "react-router-dom";
import Header from "./Header";
import Footer from "./footer";
import { BackgroundFX } from "./BackgroundFX";

export default function Layout() {
    return (
        <>
            <div className="relative flex min-h-screen flex-col text-white">
                <Header />
                <BackgroundFX intensity={1} />
                <main className="flex flex-1 flex-col">
                    <Outlet />
                </main>
                <Footer />
            </div>
        </>
    );
}
