export default function LoadingDots() {
    return (
        <span className="mt-3 flex items-center gap-1">
            <span className="h-2 w-2 animate-bounce rounded-full bg-loading-primary [animation-delay:-0.2s]" />
            <span className="h-2 w-2 animate-bounce rounded-full bg-loading-secondary [animation-delay:-0.1s]" />
            <span className="h-2 w-2 animate-bounce rounded-full bg-loading-tertiary" />
        </span>
    );
}
