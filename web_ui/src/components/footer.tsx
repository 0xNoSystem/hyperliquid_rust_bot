import React from "react";
import { BackgroundFX } from "../components/BackgroundFX";

const Footer: React.FC = () => {
    return (
        <footer className="border-accent-brand-deep text-app-text border-t py-10 text-center text-base font-semibold">
            <BackgroundFX intensity={1} />© {new Date().getFullYear()} Kwant
        </footer>
    );
};

export default Footer;
