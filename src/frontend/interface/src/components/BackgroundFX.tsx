import React from "react";

/**
 * Layered moving grid/glow background.
 * Place inside a container with `relative` (or use the fixed variant below).
 */
export function BackgroundFX({
  className = "",
  intensity = 1, // 0.0–1.0 overall opacity multiplier
}: { className?: string; intensity?: number }) {
  // clamp
  const i = Math.max(0, Math.min(1, intensity));
  return (
    <div className={`pointer-events-none absolute inset-0 overflow-hidden ${className}`}>
      {/* radial glows */}
      <div
        className="absolute inset-0"
        style={{ opacity: 0.08 * i }}
      >
        <div className="[background:radial-gradient(60%_60%_at_0%_0%,rgba(56,189,248,0.5),transparent_60%),radial-gradient(50%_50%_at_100%_0%,rgba(232,121,249,0.5),transparent_60%),radial-gradient(60%_60%_at_50%_100%,rgba(52,211,153,0.4),transparent_60%)] h-full w-full" />
      </div>

      {/* subtle grid */}
      <div
        className="absolute inset-0"
        style={{ opacity: 0.06 * i }}
      >
        <div className="bg-[linear-gradient(transparent_23px,rgba(255,255,255,0.06)_24px),linear-gradient(90deg,transparent_23px,rgba(255,255,255,0.06)_24px)] bg-[size:26px_26px] h-full w-full" />
      </div>

      {/* scanning lines */}
      <div className="absolute inset-0 [mask-image:linear-gradient(to_bottom,transparent,black_20%,black_80%,transparent)]">
        <div
          className="h-[200%] w-[200%] -translate-x-1/4 animate-scan bg-[repeating-linear-gradient(90deg,transparent_0,transparent_48px,rgba(255,255,255,0.04)_49px,rgba(255,255,255,0.04)_50px)]"
          style={{ opacity: 1 * i }}
        />
      </div>
    </div>
  );
}

/**
 * Fixed, app-wide background (sits once under the whole app).
 * Drop inside Layout to cover all routes.
 */
export function AppBackgroundFX({ intensity = 1 }: { intensity?: number }) {
  return (
    <div className="pointer-events-none fixed inset-0 -z-10">
      <BackgroundFX intensity={intensity} />
    </div>
  );
}

