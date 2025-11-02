import React from 'react';

const Footer: React.FC = () => {
  return (
    <footer className="text-center text-white font-semibold text-base py-8 bg-[#07090B] border-t border-orange-600 py-10">
      © {new Date().getFullYear()} Kwant
    </footer>
  );
};

export default Footer;

