import {
    useEffect,
    useMemo,
    useRef,
    useState,
    type KeyboardEvent as ReactKeyboardEvent,
} from "react";
import { Check, ChevronDown, Search } from "lucide-react";

export interface SearchBarOption {
    value: string;
    label: string;
    searchText?: string;
}

interface SearchBarProps {
    value: string;
    onChange: (value: string) => void;
    options: SearchBarOption[];
    placeholder?: string;
    searchPlaceholder?: string;
    emptyMessage?: string;
    ariaLabel?: string;
    align?: "left" | "right";
    containerClassName?: string;
    buttonClassName?: string;
    popoverClassName?: string;
}

const buttonBaseClass =
    "flex w-full items-center justify-between gap-3 rounded-lg border border-line-subtle bg-app-surface-3 px-3 py-2 text-left text-app-text transition hover:bg-ink-10 focus:outline-none focus:ring-2 focus:ring-accent-brand-soft/30";
const popoverBaseClass =
    "absolute top-full z-50 mt-2 w-full rounded-xl border border-line-solid bg-surface-popover p-2 shadow-2xl";
const searchInputClass =
    "w-full rounded-lg border border-line-solid bg-app-surface-3 py-2 pr-3 pl-9 text-sm text-app-text placeholder:text-app-text/40 focus:outline-none focus:ring-2 focus:ring-accent-brand-soft/30";

export default function SearchBar({
    value,
    onChange,
    options,
    placeholder = "Select an option",
    searchPlaceholder = "Search...",
    emptyMessage = "No matches found.",
    ariaLabel,
    align = "left",
    containerClassName = "",
    buttonClassName = "",
    popoverClassName = "",
}: SearchBarProps) {
    const containerRef = useRef<HTMLDivElement>(null);
    const inputRef = useRef<HTMLInputElement>(null);
    const [isOpen, setIsOpen] = useState(false);
    const [query, setQuery] = useState("");
    const [highlightedIndex, setHighlightedIndex] = useState(0);

    const selectedOption = useMemo(
        () => options.find((option) => option.value === value),
        [options, value]
    );

    const filteredOptions = useMemo(() => {
        const normalizedQuery = query.trim().toLowerCase();
        if (!normalizedQuery) {
            return options;
        }

        return options.filter((option) =>
            [option.label, option.value, option.searchText ?? ""]
                .join(" ")
                .toLowerCase()
                .includes(normalizedQuery)
        );
    }, [options, query]);

    useEffect(() => {
        if (!isOpen) {
            setQuery("");
            return;
        }

        const selectedIndex = filteredOptions.findIndex(
            (option) => option.value === value
        );
        setHighlightedIndex(selectedIndex >= 0 ? selectedIndex : 0);

        const frame = window.requestAnimationFrame(() => {
            inputRef.current?.focus();
        });

        return () => window.cancelAnimationFrame(frame);
    }, [filteredOptions, isOpen, value]);

    useEffect(() => {
        if (!isOpen) return;

        const handlePointerDown = (event: MouseEvent | TouchEvent) => {
            if (
                containerRef.current &&
                !containerRef.current.contains(event.target as Node)
            ) {
                setIsOpen(false);
            }
        };

        document.addEventListener("mousedown", handlePointerDown);
        document.addEventListener("touchstart", handlePointerDown);

        return () => {
            document.removeEventListener("mousedown", handlePointerDown);
            document.removeEventListener("touchstart", handlePointerDown);
        };
    }, [isOpen]);

    const selectOption = (option: SearchBarOption) => {
        onChange(option.value);
        setIsOpen(false);
        setQuery("");
    };

    const handleButtonKeyDown = (
        event: ReactKeyboardEvent<HTMLButtonElement>
    ) => {
        if (
            event.key === "ArrowDown" ||
            event.key === "ArrowUp" ||
            event.key === "Enter" ||
            event.key === " "
        ) {
            event.preventDefault();
            setIsOpen(true);
        }
    };

    const handleInputKeyDown = (
        event: ReactKeyboardEvent<HTMLInputElement>
    ) => {
        if (event.key === "ArrowDown") {
            event.preventDefault();
            setHighlightedIndex((prev) =>
                filteredOptions.length === 0
                    ? 0
                    : (prev + 1) % filteredOptions.length
            );
            return;
        }

        if (event.key === "ArrowUp") {
            event.preventDefault();
            setHighlightedIndex((prev) =>
                filteredOptions.length === 0
                    ? 0
                    : (prev - 1 + filteredOptions.length) %
                      filteredOptions.length
            );
            return;
        }

        if (event.key === "Enter") {
            event.preventDefault();
            const option = filteredOptions[highlightedIndex];
            if (option) {
                selectOption(option);
            }
            return;
        }

        if (event.key === "Escape" || event.key === "Tab") {
            setIsOpen(false);
        }
    };

    const displayLabel = selectedOption?.label ?? value;

    return (
        <div
            ref={containerRef}
            className={`relative w-full ${containerClassName}`.trim()}
        >
            <button
                type="button"
                aria-haspopup="listbox"
                aria-expanded={isOpen}
                aria-label={ariaLabel ?? placeholder}
                onClick={() => setIsOpen((prev) => !prev)}
                onKeyDown={handleButtonKeyDown}
                className={`${buttonBaseClass} ${buttonClassName}`.trim()}
            >
                <span
                    className={`truncate ${
                        displayLabel ? "text-app-text" : "text-app-text/50"
                    }`}
                >
                    {displayLabel || placeholder}
                </span>
                <ChevronDown
                    className={`text-app-text/50 h-4 w-4 shrink-0 transition-transform ${
                        isOpen ? "rotate-180" : ""
                    }`}
                />
            </button>

            {isOpen && (
                <div
                    className={`${popoverBaseClass} ${
                        align === "right" ? "right-0" : "left-0"
                    } ${popoverClassName}`.trim()}
                >
                    <div className="relative">
                        <Search className="text-app-text/40 pointer-events-none absolute top-1/2 left-3 h-4 w-4 -translate-y-1/2" />
                        <input
                            ref={inputRef}
                            type="text"
                            value={query}
                            onChange={(event) => {
                                setQuery(event.target.value);
                                setHighlightedIndex(0);
                            }}
                            onKeyDown={handleInputKeyDown}
                            placeholder={searchPlaceholder}
                            className={searchInputClass}
                        />
                    </div>

                    <div className="mt-2 max-h-64 overflow-y-auto">
                        {filteredOptions.length === 0 ? (
                            <div className="text-app-text/50 px-3 py-4 text-sm">
                                {emptyMessage}
                            </div>
                        ) : (
                            filteredOptions.map((option, index) => {
                                const isSelected = option.value === value;
                                const isHighlighted =
                                    index === highlightedIndex;

                                return (
                                    <button
                                        key={option.value}
                                        type="button"
                                        onMouseDown={(event) =>
                                            event.preventDefault()
                                        }
                                        onMouseEnter={() =>
                                            setHighlightedIndex(index)
                                        }
                                        onClick={() => selectOption(option)}
                                        className={`flex w-full items-center justify-between gap-3 rounded-lg px-3 py-2 text-left text-sm transition ${
                                            isHighlighted
                                                ? "bg-ink-10"
                                                : "hover:bg-ink-10"
                                        } ${
                                            isSelected
                                                ? "text-accent-brand font-semibold"
                                                : "text-app-text"
                                        }`}
                                    >
                                        <span className="truncate">
                                            {option.label}
                                        </span>
                                        <Check
                                            className={`h-4 w-4 shrink-0 ${
                                                isSelected
                                                    ? "opacity-100"
                                                    : "opacity-0"
                                            }`}
                                        />
                                    </button>
                                );
                            })
                        )}
                    </div>
                </div>
            )}
        </div>
    );
}
