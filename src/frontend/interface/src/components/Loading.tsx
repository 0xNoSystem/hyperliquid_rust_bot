import React from "react";

export default function LoadingDots() {
  return (
    <span className="flex gap-1 items-center mt-3">
      <span className="h-2 w-2 animate-bounce rounded-full bg-orange-400 [animation-delay:-0.2s]" />
      <span className="h-2 w-2 animate-bounce rounded-full bg-yellow-400 [animation-delay:-0.1s]" />
      <span className="h-2 w-2 animate-bounce rounded-full bg-yellow-100" />
    </span>
  );
}

