/**
 * Layered moving grid/glow background.
 * Place inside a container with `relative` (or use the fixed variant below).
 */
export function BackgroundFX({
    className = "",
    intensity = 1, // 0.0–1.0 overall opacity multiplier
}: {
    className?: string;
    intensity?: number;
}) {
    // clamp
    const i = Math.max(0, Math.min(1, intensity));
    return (
        <div
            className={`pointer-events-none absolute inset-0 overflow-hidden ${className}`}
        >
            {/* radial glows */}
            <div className="absolute inset-0" style={{ opacity: 0.08 * i }}>
                <div />
            </div>

            {/* subtle grid */}
            <div className="absolute inset-0" style={{ opacity: 0.06 * i }}>
                <div className="fx-grid h-full w-full" />
            </div>

            {/* scanning lines */}
            <div className="absolute inset-0 [mask-image:linear-gradient(to_bottom,transparent,black_20%,black_80%,transparent)]">
                <div
                    className="animate-scan fx-scan h-[200%] w-[200%] -translate-x-1/4"
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
