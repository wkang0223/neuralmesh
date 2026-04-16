"use client";

import { useState, useEffect, useCallback } from "react";
import { api, type Provider } from "@/lib/api-client";
import { cn, stateColor, runtimeShort, trustStars } from "@/lib/utils";
import { Search, Filter, Zap, RefreshCw, ChevronRight } from "lucide-react";
import Link from "next/link";

const RUNTIMES = ["all", "mlx", "torch-mps", "onnx-coreml", "llama-cpp"];
const SORT_OPTIONS = [
  { value: "price", label: "Price (low to high)" },
  { value: "ram",   label: "Memory (high to low)" },
  { value: "trust", label: "Trust score" },
];

export default function MarketPage() {
  const [providers, setProviders]   = useState<Provider[]>([]);
  const [loading, setLoading]       = useState(true);
  const [error, setError]           = useState<string | null>(null);
  const [minRam, setMinRam]         = useState<number>(0);
  const [runtime, setRuntime]       = useState("all");
  const [maxPrice, setMaxPrice]     = useState<number>(10);
  const [sort, setSort]             = useState("price");
  const [search, setSearch]         = useState("");
  const [total, setTotal]           = useState(0);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const { providers: list, total: t } = await api.listProviders({
        min_ram_gb: minRam || undefined,
        runtime: runtime !== "all" ? runtime : undefined,
        max_price: maxPrice < 10 ? maxPrice : undefined,
        sort,
        limit: 50,
      });
      setProviders(list);
      setTotal(t);
    } catch {
      setError("Cannot reach coordinator. Check that the backend is running.");
    } finally {
      setLoading(false);
    }
  }, [minRam, runtime, maxPrice, sort]);

  useEffect(() => { load(); }, [load]);

  const filtered = providers.filter((p) => {
    if (!search) return true;
    return (
      p.chip_model.toLowerCase().includes(search.toLowerCase()) ||
      p.region?.toLowerCase().includes(search.toLowerCase()) ||
      p.id.includes(search)
    );
  });

  return (
    <div className="min-h-screen pt-14">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 py-8">

        {/* Header */}
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 mb-8">
          <div>
            <h1 className="text-2xl font-bold text-white">GPU Marketplace</h1>
            <p className="text-slate-400 text-sm mt-1">
              {total > 0 ? `${total} providers available` : "Browse available Apple Silicon providers"}
            </p>
          </div>
          <button
            onClick={load}
            className="flex items-center gap-2 px-3 py-1.5 rounded-lg border border-slate-700 text-slate-400 hover:text-white hover:border-slate-600 text-sm transition-colors"
          >
            <RefreshCw className={cn("h-3.5 w-3.5", loading && "animate-spin")} />
            Refresh
          </button>
        </div>

        {/* Filters */}
        <div className="glass rounded-xl p-4 mb-6">
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-5 gap-3">

            {/* Search */}
            <div className="lg:col-span-2 relative">
              <Search className="absolute left-3 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-slate-500" />
              <input
                type="text"
                placeholder="Search chip, region, ID..."
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                className="w-full pl-8 pr-3 py-2 rounded-lg bg-slate-900 border border-slate-700 text-sm text-slate-300 placeholder-slate-500 focus:outline-none focus:border-brand-400/50"
              />
            </div>

            {/* Min RAM */}
            <div>
              <label className="text-xs text-slate-500 block mb-1">Min memory (GB)</label>
              <select
                value={minRam}
                onChange={(e) => setMinRam(Number(e.target.value))}
                className="w-full px-3 py-2 rounded-lg bg-slate-900 border border-slate-700 text-sm text-slate-300 focus:outline-none focus:border-brand-400/50"
              >
                {[0, 16, 24, 48, 64, 96, 128, 192].map((v) => (
                  <option key={v} value={v}>{v === 0 ? "Any" : `${v} GB`}</option>
                ))}
              </select>
            </div>

            {/* Runtime */}
            <div>
              <label className="text-xs text-slate-500 block mb-1">Runtime</label>
              <select
                value={runtime}
                onChange={(e) => setRuntime(e.target.value)}
                className="w-full px-3 py-2 rounded-lg bg-slate-900 border border-slate-700 text-sm text-slate-300 focus:outline-none focus:border-brand-400/50"
              >
                {RUNTIMES.map((r) => (
                  <option key={r} value={r}>{r === "all" ? "All runtimes" : runtimeShort(r)}</option>
                ))}
              </select>
            </div>

            {/* Sort */}
            <div>
              <label className="text-xs text-slate-500 block mb-1">Sort by</label>
              <select
                value={sort}
                onChange={(e) => setSort(e.target.value)}
                className="w-full px-3 py-2 rounded-lg bg-slate-900 border border-slate-700 text-sm text-slate-300 focus:outline-none focus:border-brand-400/50"
              >
                {SORT_OPTIONS.map((o) => (
                  <option key={o.value} value={o.value}>{o.label}</option>
                ))}
              </select>
            </div>
          </div>
        </div>

        {/* Provider grid */}
        {error ? (
          <div className="text-center py-20 text-slate-500">
            <RefreshCw className="h-8 w-8 mx-auto mb-3 opacity-30" />
            <p className="text-red-400">{error}</p>
            <button onClick={load} className="mt-3 text-sm text-brand-400 hover:text-brand-300">
              Retry
            </button>
          </div>
        ) : loading ? (
          <div className="text-center py-20 text-slate-500">
            <RefreshCw className="h-8 w-8 mx-auto mb-3 opacity-30 animate-spin" />
            <p>Connecting to coordinator…</p>
          </div>
        ) : filtered.length === 0 ? (
          <div className="text-center py-20 text-slate-500">
            <Filter className="h-8 w-8 mx-auto mb-3 opacity-30" />
            {providers.length === 0 ? (
              <p>No providers connected yet. Be the first to <a href="/account" className="text-brand-400 hover:text-brand-300">register your Mac</a>.</p>
            ) : (
              <>
                <p>No providers match your filters.</p>
                <button
                  onClick={() => { setMinRam(0); setRuntime("all"); setSearch(""); }}
                  className="mt-3 text-sm text-brand-400 hover:text-brand-300"
                >
                  Clear filters
                </button>
              </>
            )}
          </div>
        ) : (
          <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
            {filtered.map((p) => (
              <ProviderCard key={p.id} provider={p} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function ProviderCard({ provider: p }: { provider: Provider }) {
  const isAvailable = p.state === "available";

  return (
    <div className={cn(
      "glass rounded-xl p-5 flex flex-col gap-4 hover:border-slate-700 transition-all",
      isAvailable && "hover:border-brand-400/30"
    )}>
      {/* Header */}
      <div className="flex items-start justify-between gap-2">
        <div>
          <div className="font-semibold text-white text-sm">{p.chip_model}</div>
          <div className="text-xs text-slate-500 font-mono mt-0.5">{p.id.slice(0, 12)}…</div>
        </div>
        <span className={cn("text-xs px-2 py-0.5 rounded-full font-medium", stateColor(p.state))}>
          {p.state}
        </span>
      </div>

      {/* Stats grid */}
      <div className="grid grid-cols-3 gap-2">
        <div className="bg-slate-900/60 rounded-lg p-2.5 text-center">
          <div className="text-lg font-bold text-white font-mono">{p.unified_memory_gb}</div>
          <div className="text-xs text-slate-500">GB unified</div>
        </div>
        <div className="bg-slate-900/60 rounded-lg p-2.5 text-center">
          <div className="text-lg font-bold text-white font-mono">{p.gpu_cores}</div>
          <div className="text-xs text-slate-500">GPU cores</div>
        </div>
        <div className="bg-slate-900/60 rounded-lg p-2.5 text-center">
          <div className="text-lg font-bold text-green-400 font-mono">
            ${p.floor_price_nmc_per_hour.toFixed(2)}
          </div>
          <div className="text-xs text-slate-500">HC/hr</div>
        </div>
      </div>

      {/* Runtimes */}
      <div className="flex flex-wrap gap-1.5">
        {p.installed_runtimes.map((rt) => (
          <span
            key={rt}
            className="text-xs px-2 py-0.5 rounded bg-slate-900 border border-slate-700 text-slate-400"
          >
            {runtimeShort(rt)}
          </span>
        ))}
      </div>

      {/* Footer */}
      <div className="flex items-center justify-between pt-1 border-t border-slate-800">
        <div className="text-xs text-yellow-500">
          {trustStars(p.trust_score)}
          <span className="text-slate-500 ml-1">{p.trust_score.toFixed(1)}</span>
        </div>
        {p.region && (
          <span className="text-xs text-slate-500">{p.region}</span>
        )}
        <Link
          href={`/jobs?provider=${p.id}&runtime=${p.installed_runtimes[0]}&ram=${p.unified_memory_gb}`}
          className={cn(
            "flex items-center gap-1 text-xs font-medium px-2.5 py-1 rounded-lg transition-colors",
            isAvailable
              ? "bg-brand-400/10 text-brand-400 hover:bg-brand-400/20 border border-brand-400/20"
              : "bg-slate-800 text-slate-500 cursor-not-allowed"
          )}
          aria-disabled={!isAvailable}
        >
          <Zap className="h-3 w-3" />
          Use
          <ChevronRight className="h-3 w-3" />
        </Link>
      </div>
    </div>
  );
}
