import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatNmc(amount: number, decimals = 4): string {
  return amount.toFixed(decimals) + " HC";
}

export function formatRam(gb: number): string {
  return gb >= 1024 ? `${(gb / 1024).toFixed(1)} TB` : `${gb} GB`;
}

export function trustStars(score: number): string {
  const full = Math.round(Math.min(score, 5));
  return "★".repeat(full) + "☆".repeat(5 - full);
}

export function runtimeShort(rt: string): string {
  const map: Record<string, string> = {
    "mlx": "MLX",
    "torch-mps": "MPS",
    "onnx-coreml": "ONNX",
    "llama-cpp": "Llama",
    "shell": "Shell",
  };
  return map[rt] ?? rt;
}

export function stateColor(state: string): string {
  const map: Record<string, string> = {
    running:   "text-green-400 bg-green-400/10",
    complete:  "text-blue-400 bg-blue-400/10",
    failed:    "text-red-400 bg-red-400/10",
    cancelled: "text-slate-400 bg-slate-400/10",
    queued:    "text-yellow-400 bg-yellow-400/10",
    matching:  "text-yellow-400 bg-yellow-400/10",
    assigned:  "text-brand-400 bg-brand-400/10",
    available: "text-green-400 bg-green-400/10",
    leased:    "text-yellow-400 bg-yellow-400/10",
    offline:   "text-slate-500 bg-slate-500/10",
  };
  return map[state] ?? "text-slate-400 bg-slate-400/10";
}
