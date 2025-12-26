export default function LoadingDots() {
    return (
        <span className="mt-3 flex items-center gap-1">
            <span className="bg-loading-primary h-2 w-2 animate-bounce rounded-full [animation-delay:-0.2s]" />
            <span className="bg-loading-secondary h-2 w-2 animate-bounce rounded-full [animation-delay:-0.1s]" />
            <span className="bg-loading-tertiary h-2 w-2 animate-bounce rounded-full" />
        </span>
    );
}
