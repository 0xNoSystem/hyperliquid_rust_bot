import { useEffect, useRef } from "react";

const FPS = 80;

// ---------- types ----------
type Vec3 = { x: number; y: number; z: number };
type Vec2 = { x: number; y: number };
type Face = number[];

/*
type TorusOptions = {
    segU?: number;
    segV?: number;
    R?: number;
    r?: number;
};
function _makeTorus({
    segU = 24,
    segV = 12,
    R = 0.65,
    r = 0.22,
}: TorusOptions = {}): { vs: Vec3[]; fs: Face[] } {
    const vs: Vec3[] = [];
    const fs: Face[] = [];

    const idx = (u: number, v: number) => u * segV + v;

    for (let u = 0; u < segU; u++) {
        const a = (u / segU) * Math.PI * 2;
        const ca = Math.cos(a);
        const sa = Math.sin(a);

        for (let v = 0; v < segV; v++) {
            const b = (v / segV) * Math.PI * 2;
            const cb = Math.cos(b);
            const sb = Math.sin(b);

            vs.push({
                x: (R + r * cb) * ca,
                y: r * sb,
                z: (R + r * cb) * sa,
            });
        }
    }

    for (let u = 0; u < segU; u++) {
        const un = (u + 1) % segU;
        for (let v = 0; v < segV; v++) {
            const vn = (v + 1) % segV;
            fs.push([idx(u, v), idx(un, v), idx(un, vn), idx(u, vn)]);
        }
    }

    return { vs, fs };
}
*/

const cubeVs: Vec3[] = [
    { x: 0.25, y: 0.25, z: 0.25 },
    { x: -0.25, y: 0.25, z: 0.25 },
    { x: -0.25, y: -0.25, z: 0.25 },
    { x: 0.25, y: -0.25, z: 0.25 },
    { x: 0.25, y: 0.25, z: -0.25 },
    { x: -0.25, y: 0.25, z: -0.25 },
    { x: -0.25, y: -0.25, z: -0.25 },
    { x: 0.25, y: -0.25, z: -0.25 },
];

const cubeFs: Face[] = [
    [0, 1, 2, 3],
    [4, 5, 6, 7],
    [0, 4],
    [1, 5],
    [2, 6],
    [3, 7],
];

// ---------- component ----------

type RotatingCubeProps = {
    size?: number;
    foreground?: string;
    background?: string;
};

export default function RotatingCube({
    size = 50,
    foreground = "#FF6900",
    background = "transparent",
}: RotatingCubeProps) {
    const canvasRef = useRef<HTMLCanvasElement | null>(null);
    const hoveringRef = useRef<boolean>(false);

    const vs = cubeVs;
    const fs = cubeFs;
    // const { vs, fs } = makeTorus(); // swap geometry here

    useEffect(() => {
        const canvas = canvasRef.current;
        if (!canvas) return;

        canvas.width = size;
        canvas.height = size;

        const ctx = canvas.getContext("2d", { alpha: true });
        if (!ctx) return;

        const clear = () => {
            if (background !== "transparent") {
                ctx.fillStyle = background;
                ctx.fillRect(0, 0, size, size);
            } else {
                ctx.clearRect(0, 0, size, size);
            }
        };

        const line = (a: Vec2, b: Vec2) => {
            ctx.lineWidth = 0.7;
            ctx.strokeStyle = foreground;
            ctx.beginPath();
            ctx.moveTo(a.x, a.y);
            ctx.lineTo(b.x, b.y);
            ctx.stroke();
        };

        const screen = (p: Vec2): Vec2 => ({
            x: ((p.x + 1) / 2) * size,
            y: (1 - (p.y + 1) / 2) * size,
        });

        const project = ({ x, y, z }: Vec3): Vec2 => ({
            x: x / z,
            y: y / z,
        });

        const translateZ = ({ x, y, z }: Vec3, dz: number): Vec3 => ({
            x,
            y,
            z: z + dz,
        });

        const rotateXZ = ({ x, y, z }: Vec3, a: number): Vec3 => {
            const c = Math.cos(a);
            const s = Math.sin(a);
            return {
                x: x * c - z * s,
                y,
                z: x * s + z * c,
            };
        };

        const onEnter = () => (hoveringRef.current = true);
        const onLeave = () => (hoveringRef.current = false);

        canvas.addEventListener("mouseenter", onEnter);
        canvas.addEventListener("mouseleave", onLeave);
        canvas.style.cursor = "pointer";

        let angle = 0;
        let running = true;

        const frame = () => {
            if (!running) return;

            if (!hoveringRef.current) {
                angle += Math.PI / FPS;
            }

            clear();

            for (const f of fs) {
                for (let i = 0; i < f.length; i++) {
                    const a = vs[f[i]];
                    const b = vs[f[(i + 1) % f.length]];

                    line(
                        screen(project(translateZ(rotateXZ(a, angle), 1))),
                        screen(project(translateZ(rotateXZ(b, angle), 1)))
                    );
                }
            }

            requestAnimationFrame(frame);
        };

        frame();

        return () => {
            running = false;
            canvas.removeEventListener("mouseenter", onEnter);
            canvas.removeEventListener("mouseleave", onLeave);
        };
    }, [size, foreground, background, fs, vs]);

    return <canvas ref={canvasRef} />;
}
