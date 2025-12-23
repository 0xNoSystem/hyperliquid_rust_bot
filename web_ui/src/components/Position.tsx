import type { OpenPositionLocal } from "../types";
import { computeUPnL, num } from "../types";

interface PositionTableProps {
    position: OpenPositionLocal;
    price: number | null;
    lev: number | null;
    szDecimals: number;
    formatPrice: (n: number) => string;
}

const PositionTable = ({
    position,
    price,
    lev,
    szDecimals,
    formatPrice,
}: PositionTableProps) => {
    return (
        <table className="min-w-full text-[11px]">
            <thead className="text-white/60">
                <tr>
                    <th className="py-2 pr-2 text-left">Side</th>

                    <th className="py-2 pr-2 text-right">Entry</th>

                    <th className="py-2 pr-2 text-right">Size</th>

                    <th className="py-2 pr-2 text-right">Funding</th>

                    <th className="py-2 text-right">UPNL</th>
                </tr>
            </thead>

            <tbody>
                <tr className="border-t border-white/10">
                    <td
                        className={`py-2 pr-4 font-semibold uppercase ${
                            position.side === "long"
                                ? "text-green-500"
                                : "text-red-500"
                        }`}
                    >
                        {position.side}
                    </td>

                    <td className="py-2 pr-2 text-right">
                        {formatPrice(position.entryPx)}
                    </td>

                    <td className="py-2 pr-2 text-right">
                        {num(position.size, szDecimals)}
                    </td>

                    <td className="py-2 pr-2 text-right">
                        {num(position.funding, 2)}$
                    </td>

                    <td className="py-2 text-right text-orange-400">
                        {price == null || lev == null
                            ? "—"
                            : (() => {
                                  const [upnl, change] = computeUPnL(
                                      position,
                                      price,
                                      lev
                                  );

                                  return `${num(upnl, 2)}$ (${num(
                                      change * 100,
                                      2
                                  )}%)`;
                              })()}
                    </td>
                </tr>
            </tbody>
        </table>
    );
};

export default PositionTable;
