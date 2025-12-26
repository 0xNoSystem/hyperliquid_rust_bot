import React from "react";
import { BackgroundFX } from "../components/BackgroundFX";

const Footer: React.FC = () => {
    return (
        <footer className="border-t border-accent-brand-deep py-10 text-center text-base font-semibold text-app-text">
            <BackgroundFX intensity={1} />© {new Date().getFullYear()} Kwant
        </footer>
    );
};

export default Footer;
