export default function LoadingDots() {
    return (
        <span className="mt-3 flex items-center gap-1">
            <span className="h-2 w-2 animate-bounce rounded-full bg-orange-400 [animation-delay:-0.2s]" />
            <span className="h-2 w-2 animate-bounce rounded-full bg-yellow-400 [animation-delay:-0.1s]" />
            <span className="h-2 w-2 animate-bounce rounded-full bg-yellow-200" />
        </span>
    );
}
