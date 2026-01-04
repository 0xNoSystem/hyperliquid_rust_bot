import React from "react";

const Footer: React.FC = () => {
    return (
        <footer className="border-accent-brand-deep text-app-text border-t py-10 text-center text-base font-semibold">
            © {new Date().getFullYear()} Kwant
        </footer>
    );
};

export default Footer;
