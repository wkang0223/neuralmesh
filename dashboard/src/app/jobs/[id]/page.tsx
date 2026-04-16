"use client";

import { useEffect, useRef, useState } from "react";
import { use } from "react";
import { api, type Job } from "@/lib/api-client";
import { cn, stateColor, runtimeShort } from "@/lib/utils";
import { Terminal, RefreshCw, XCircle, ChevronLeft, Clock } from "lucide-react";
import Link from "next/link";
import { toast } from "sonner";

export default function JobDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = use(params);
  const [job, setJob]           = useState<Job | null>(null);
  const [logs, setLogs]         = useState<string>("");
  const [following, setFollowing] = useState(false);
  const [loading, setLoading]   = useState(true);
  const [cancelling, setCancelling] = useState(false);
  const logRef = useRef<HTMLPreElement>(null);
  const followRef = useRef(false);

  // Load job details
  useEffect(() => {
    api.getJob(id)
      .then(setJob)
      .catch(() => {})
      .finally(() => setLoading(false));
  }, [id]);

  // Log streaming
  useEffect(() => {
    let offset = 0;
    let stopped = false;

    async function fetchLogs() {
      try {
        const result = await api.getJobLogs(id, offset);
        if (result.output) {
          setLogs((prev) => prev + result.output);
          offset += result.output.length;
          // Auto-scroll
          if (logRef.current) {
            logRef.current.scrollTop = logRef.current.scrollHeight;
          }
        }
        if (result.is_complete) {
          setFollowing(false);
          return;
        }
      } catch { /* ignore */ }
      if (!stopped && followRef.current) {
        setTimeout(fetchLogs, 2000);
      }
    }

    // Initial fetch
    fetchLogs();

    return () => { stopped = true; };
  }, [id]);

  function startFollow() {
    followRef.current = true;
    setFollowing(true);
  }
  function stopFollow() {
    followRef.current = false;
    setFollowing(false);
  }

  async function cancelJob() {
    if (!confirm("Cancel this job? Running work will be stopped.")) return;
    setCancelling(true);
    try {
      await api.cancelJob(id);
      toast.success("Job cancelled");
      setJob((j) => j ? { ...j, state: "cancelled" } : j);
    } catch (err: unknown) {
      toast.error(err instanceof Error ? err.message : "Failed to cancel");
    } finally {
      setCancelling(false);
    }
  }

  if (loading) {
    return (
      <div className="min-h-screen pt-14 flex items-center justify-center">
        <RefreshCw className="h-6 w-6 text-brand-400 animate-spin" />
      </div>
    );
  }

  return (
    <div className="min-h-screen pt-14">
      <div className="max-w-5xl mx-auto px-4 sm:px-6 py-8">

        {/* Back */}
        <Link href="/jobs" className="inline-flex items-center gap-1.5 text-slate-400 hover:text-white text-sm mb-6 transition-colors">
          <ChevronLeft className="h-4 w-4" /> Back to Jobs
        </Link>

        {/* Header */}
        <div className="flex flex-col sm:flex-row sm:items-start justify-between gap-4 mb-6">
          <div>
            <h1 className="text-xl font-bold text-white font-mono">{id}</h1>
            {job && (
              <div className="flex items-center gap-3 mt-2">
                <span className={cn("text-xs px-2 py-0.5 rounded-full", stateColor(job.state))}>
                  {job.state}
                </span>
                <span className="text-xs text-slate-500">
                  {runtimeShort(job.runtime)} · {job.min_ram_gb} GB
                </span>
                <span className="text-xs text-slate-500">
                  Created {new Date(job.created_at).toLocaleString()}
                </span>
              </div>
            )}
          </div>

          <div className="flex items-center gap-2">
            {/* Follow toggle */}
            <button
              onClick={following ? stopFollow : startFollow}
              className={cn(
                "flex items-center gap-1.5 px-3 py-1.5 rounded-lg border text-sm font-medium transition-colors",
                following
                  ? "border-brand-400/50 bg-brand-400/10 text-brand-400"
                  : "border-slate-700 text-slate-400 hover:text-white hover:border-slate-600"
              )}
            >
              <Clock className={cn("h-3.5 w-3.5", following && "animate-pulse")} />
              {following ? "Following" : "Follow logs"}
            </button>

            {/* Cancel */}
            {job && !["complete", "failed", "cancelled"].includes(job.state) && (
              <button
                onClick={cancelJob}
                disabled={cancelling}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-red-400/30 text-red-400 hover:bg-red-400/10 text-sm transition-colors disabled:opacity-50"
              >
                <XCircle className="h-3.5 w-3.5" />
                Cancel
              </button>
            )}
          </div>
        </div>

        {/* Job details */}
        {job && (
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 mb-6">
            {[
              { label: "Runtime",  value: runtimeShort(job.runtime) },
              { label: "Min RAM",  value: `${job.min_ram_gb} GB` },
              { label: "Max price",value: `${job.max_price_per_hour} HC/hr` },
              { label: "Actual cost", value: job.actual_cost_nmc ? `${job.actual_cost_nmc.toFixed(4)} HC` : "—" },
            ].map((item) => (
              <div key={item.label} className="glass rounded-lg px-3 py-2.5">
                <div className="text-xs text-slate-500">{item.label}</div>
                <div className="text-sm font-medium text-white mt-0.5">{item.value}</div>
              </div>
            ))}
          </div>
        )}

        {/* Terminal */}
        <div className="glass rounded-xl overflow-hidden">
          <div className="flex items-center gap-2 px-4 py-2.5 border-b border-slate-800 bg-slate-900/50">
            <Terminal className="h-3.5 w-3.5 text-brand-400" />
            <span className="text-xs font-medium text-slate-400">stdout / stderr</span>
            {following && (
              <span className="ml-auto flex items-center gap-1 text-xs text-brand-400">
                <span className="h-1.5 w-1.5 rounded-full bg-brand-400 animate-pulse" />
                Live
              </span>
            )}
          </div>
          <pre
            ref={logRef}
            className="terminal min-h-80 max-h-[60vh] overflow-auto text-xs"
          >
            {logs || "(waiting for output…)"}
          </pre>
        </div>

        {/* Exit code */}
        {job?.exit_code !== undefined && (
          <div className={cn(
            "mt-4 inline-flex items-center gap-2 text-sm px-3 py-2 rounded-lg",
            job.exit_code === 0
              ? "bg-green-400/10 border border-green-400/20 text-green-400"
              : "bg-red-400/10 border border-red-400/20 text-red-400"
          )}>
            {job.exit_code === 0 ? "✓" : "✗"} Exit code {job.exit_code}
          </div>
        )}

      </div>
    </div>
  );
}
