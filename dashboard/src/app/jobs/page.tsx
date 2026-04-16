"use client";

import { Suspense, useEffect, useState, useCallback } from "react";
import { useSearchParams } from "next/navigation";
import { api, type Job } from "@/lib/api-client";
import { cn, stateColor, runtimeShort } from "@/lib/utils";
import {
  Zap, Upload, Clock, CheckCircle, XCircle, Activity,
  ChevronRight, Terminal, RefreshCw
} from "lucide-react";
import Link from "next/link";
import { toast } from "sonner";

const RUNTIMES = ["mlx", "torch-mps", "onnx-coreml", "llama-cpp", "shell"];

function JobsInner() {
  const params = useSearchParams();
  const [tab, setTab] = useState<"submit" | "list">("list");
  const [jobs, setJobs] = useState<Job[]>([]);
  const [loading, setLoading] = useState(false);

  // Pre-fill from market page URL params
  const defaultRuntime = params.get("runtime") ?? "mlx";
  const defaultRam     = parseInt(params.get("ram") ?? "16", 10);

  // Submit form state
  const [scriptName, setScriptName]   = useState("inference.py");
  const [runtime, setRuntime]         = useState(defaultRuntime);
  const [ram, setRam]                 = useState(defaultRam);
  const [hours, setHours]             = useState(1);
  const [maxPrice, setMaxPrice]       = useState(0.5);
  const [submitting, setSubmitting]   = useState(false);

  const accountId = typeof window !== "undefined"
    ? localStorage.getItem("nm_account_id") ?? ""
    : "";

  const loadJobs = useCallback(async () => {
    setLoading(true);
    try {
      const { jobs: list } = await api.listJobs(accountId, 30);
      setJobs(list);
    } catch {
      // Jobs list will remain empty; no toast needed for background refresh
    } finally {
      setLoading(false);
    }
  }, [accountId]);

  useEffect(() => { loadJobs(); }, [loadJobs]);

  // Auto-switch to submit tab if coming from market with provider pre-selected
  useEffect(() => {
    if (params.get("provider")) setTab("submit");
  }, [params]);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setSubmitting(true);
    try {
      if (!accountId) {
        toast.error("Create a device-linked account first (Account page).");
        setSubmitting(false);
        return;
      }
      const result = await api.submitJob({
        account_id: accountId,
        runtime,
        min_ram_gb: ram,
        max_duration_secs: hours * 3600,
        max_price_per_hour: maxPrice,
        // bundle_hash and bundle_url are set by the nm CLI when uploading a real script.
        // Web form submissions require the coordinator to accept a pending job without a bundle,
        // then the user uploads via: nm job attach <job_id> ./script.py
        script_name: scriptName,
      });
      toast.success(`Job submitted! ID: ${result.job_id.slice(0, 12)}…`);
      setTab("list");
      loadJobs();
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : "Job submission failed";
      toast.error(message);
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="min-h-screen pt-14">
      <div className="max-w-5xl mx-auto px-4 sm:px-6 py-8">

        {/* Header */}
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 mb-8">
          <h1 className="text-2xl font-bold text-white">Jobs</h1>
          <div className="flex items-center gap-2">
            {/* Tab switcher */}
            <div className="flex rounded-lg border border-slate-700 overflow-hidden">
              {(["list", "submit"] as const).map((t) => (
                <button
                  key={t}
                  onClick={() => setTab(t)}
                  className={cn(
                    "px-4 py-1.5 text-sm font-medium transition-colors",
                    tab === t
                      ? "bg-brand-400/10 text-brand-300"
                      : "text-slate-400 hover:text-white hover:bg-slate-800"
                  )}
                >
                  {t === "list" ? "My Jobs" : "Submit Job"}
                </button>
              ))}
            </div>
            <button
              onClick={loadJobs}
              className="p-1.5 rounded-lg border border-slate-700 text-slate-400 hover:text-white"
            >
              <RefreshCw className={cn("h-4 w-4", loading && "animate-spin")} />
            </button>
          </div>
        </div>

        {/* Submit tab */}
        {tab === "submit" && (
          <form onSubmit={handleSubmit} className="space-y-6">
            <div className="glass rounded-xl p-6">
              <h2 className="text-sm font-semibold text-white mb-5 flex items-center gap-2">
                <Zap className="h-4 w-4 text-brand-400" />
                Job Configuration
              </h2>

              <div className="grid grid-cols-1 md:grid-cols-2 gap-4">

                {/* Script name */}
                <div className="md:col-span-2">
                  <label className="text-xs text-slate-500 block mb-1.5">Script filename</label>
                  <div className="relative">
                    <Upload className="absolute left-3 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-slate-500" />
                    <input
                      type="text"
                      value={scriptName}
                      onChange={(e) => setScriptName(e.target.value)}
                      placeholder="inference.py"
                      className="w-full pl-8 pr-3 py-2.5 rounded-lg bg-slate-900 border border-slate-700 text-sm text-slate-300 placeholder-slate-500 focus:outline-none focus:border-brand-400/50"
                    />
                  </div>
                  <p className="text-xs text-slate-600 mt-1">
                    The <code className="font-mono">nm job submit ./script.py</code> CLI command bundles and uploads your file automatically.
                  </p>
                </div>

                {/* Runtime */}
                <div>
                  <label className="text-xs text-slate-500 block mb-1.5">Runtime</label>
                  <select
                    value={runtime}
                    onChange={(e) => setRuntime(e.target.value)}
                    className="w-full px-3 py-2.5 rounded-lg bg-slate-900 border border-slate-700 text-sm text-slate-300 focus:outline-none focus:border-brand-400/50"
                  >
                    {RUNTIMES.map((r) => (
                      <option key={r} value={r}>{runtimeShort(r)} — {r}</option>
                    ))}
                  </select>
                </div>

                {/* Min RAM */}
                <div>
                  <label className="text-xs text-slate-500 block mb-1.5">
                    Minimum memory — <span className="text-brand-400">{ram} GB</span>
                  </label>
                  <input
                    type="range"
                    min={8} max={192} step={8}
                    value={ram}
                    onChange={(e) => setRam(Number(e.target.value))}
                    className="w-full accent-brand-400"
                  />
                  <div className="flex justify-between text-xs text-slate-600 mt-1">
                    <span>8 GB</span><span>96 GB</span><span>192 GB</span>
                  </div>
                </div>

                {/* Hours */}
                <div>
                  <label className="text-xs text-slate-500 block mb-1.5">Max duration (hours)</label>
                  <select
                    value={hours}
                    onChange={(e) => setHours(Number(e.target.value))}
                    className="w-full px-3 py-2.5 rounded-lg bg-slate-900 border border-slate-700 text-sm text-slate-300 focus:outline-none focus:border-brand-400/50"
                  >
                    {[0.25, 0.5, 1, 2, 4, 8, 12, 24].map((h) => (
                      <option key={h} value={h}>{h < 1 ? `${h * 60} min` : `${h} hr${h > 1 ? "s" : ""}`}</option>
                    ))}
                  </select>
                </div>

                {/* Max price */}
                <div>
                  <label className="text-xs text-slate-500 block mb-1.5">
                    Max price — <span className="text-brand-400">{maxPrice.toFixed(3)} HC/hr</span>
                  </label>
                  <input
                    type="range"
                    min={0.01} max={1.0} step={0.01}
                    value={maxPrice}
                    onChange={(e) => setMaxPrice(Number(e.target.value))}
                    className="w-full accent-brand-400"
                  />
                  <div className="flex justify-between text-xs text-slate-600 mt-1">
                    <span>0.01</span><span>0.50</span><span>1.00</span>
                  </div>
                </div>
              </div>
            </div>

            {/* Cost estimate */}
            <div className="glass rounded-xl p-4 flex items-center justify-between">
              <div>
                <div className="text-xs text-slate-500">Estimated cost</div>
                <div className="text-lg font-bold text-green-400 font-mono mt-0.5">
                  ≤ {(maxPrice * hours).toFixed(4)} HC
                </div>
                <div className="text-xs text-slate-500">
                  {hours}h × {maxPrice.toFixed(3)} HC/hr max
                </div>
              </div>
              <button
                type="submit"
                disabled={submitting}
                className="flex items-center gap-2 px-6 py-2.5 rounded-lg bg-brand-400 text-slate-950 font-semibold hover:bg-brand-300 disabled:opacity-50 disabled:cursor-not-allowed transition-colors shadow-lg shadow-brand-400/20"
              >
                {submitting ? (
                  <RefreshCw className="h-4 w-4 animate-spin" />
                ) : (
                  <Zap className="h-4 w-4" />
                )}
                Submit Job
                <ChevronRight className="h-4 w-4" />
              </button>
            </div>
          </form>
        )}

        {/* List tab */}
        {tab === "list" && (
          <div className="glass rounded-xl overflow-hidden">
            <div className="px-5 py-4 border-b border-slate-800 flex items-center justify-between">
              <span className="text-sm font-semibold text-white">All Jobs</span>
              <button
                onClick={() => setTab("submit")}
                className="flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-lg bg-brand-400/10 text-brand-400 border border-brand-400/20 hover:bg-brand-400/20 transition-colors"
              >
                <Zap className="h-3 w-3" /> New Job
              </button>
            </div>

            {loading ? (
              <div className="py-12 text-center text-slate-500 text-sm">Loading jobs…</div>
            ) : jobs.length === 0 ? (
              <div className="py-12 text-center">
                <Zap className="h-8 w-8 mx-auto mb-3 text-slate-700" />
                <p className="text-slate-500 text-sm">No jobs yet.</p>
                <button
                  onClick={() => setTab("submit")}
                  className="mt-3 text-sm text-brand-400 hover:text-brand-300"
                >
                  Submit your first job →
                </button>
              </div>
            ) : (
              <div className="divide-y divide-slate-800">
                {jobs.map((job) => (
                  <div key={job.id} className="px-5 py-4 flex items-center gap-4 hover:bg-slate-900/40 transition-colors">
                    {/* Icon */}
                    <div className="flex-shrink-0">
                      {job.state === "complete"  && <CheckCircle className="h-4 w-4 text-blue-400" />}
                      {job.state === "running"   && <Activity    className="h-4 w-4 text-green-400 animate-pulse" />}
                      {job.state === "failed"    && <XCircle     className="h-4 w-4 text-red-400" />}
                      {(job.state === "queued" || job.state === "matching") && <Clock className="h-4 w-4 text-yellow-400" />}
                      {job.state === "cancelled" && <XCircle     className="h-4 w-4 text-slate-500" />}
                    </div>

                    {/* Info */}
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2">
                        <span className="font-mono text-xs text-slate-300 truncate">{job.id}</span>
                        <span className="text-xs px-1.5 py-0.5 rounded bg-slate-800 border border-slate-700 text-slate-400">
                          {runtimeShort(job.runtime)}
                        </span>
                      </div>
                      <div className="text-xs text-slate-500 mt-0.5">
                        {new Date(job.created_at).toLocaleString(undefined, {
                          month: "short", day: "numeric", hour: "2-digit", minute: "2-digit"
                        })}
                        {" · "}{job.min_ram_gb} GB
                      </div>
                    </div>

                    {/* Cost */}
                    {job.actual_cost_nmc !== undefined && (
                      <span className="text-xs font-mono text-green-400 hidden sm:block">
                        {job.actual_cost_nmc.toFixed(4)} HC
                      </span>
                    )}

                    {/* State */}
                    <span className={cn("text-xs px-2 py-0.5 rounded-full hidden md:block", stateColor(job.state))}>
                      {job.state}
                    </span>

                    {/* Logs */}
                    <Link href={`/jobs/${job.id}`} className="text-slate-500 hover:text-brand-400 transition-colors">
                      <Terminal className="h-3.5 w-3.5" />
                    </Link>
                  </div>
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

export default function JobsPage() {
  return (
    <Suspense fallback={
      <div className="min-h-screen pt-14 flex items-center justify-center text-slate-500">Loading…</div>
    }>
      <JobsInner />
    </Suspense>
  );
}
