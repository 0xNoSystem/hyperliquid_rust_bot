import React from "react";

const Footer: React.FC = () => {
    return (
        <footer className="border-t border-orange-600 bg-[#07090B] py-8 py-10 text-center text-base font-semibold text-white">
            © {new Date().getFullYear()} Kwant
        </footer>
    );
};

export default Footer;
