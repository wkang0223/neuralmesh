"use client";

import { useEffect, useState } from "react";
import { api, type Job } from "@/lib/api-client";
import { cn, stateColor, formatNmc, runtimeShort } from "@/lib/utils";
import {
  Cpu, DollarSign, Activity, Clock, CheckCircle, XCircle,
  TrendingUp, Copy, Terminal, AlertCircle
} from "lucide-react";
import { toast } from "sonner";
import {
  AreaChart, Area, XAxis, YAxis, Tooltip, ResponsiveContainer,
} from "recharts";

export default function ProviderPage() {
  const [accountId, setAccountId] = useState<string | null>(null);
  const [jobs, setJobs]           = useState<Job[]>([]);
  const [agentRunning, setAgentRunning] = useState<boolean | null>(null);
  const [totalEarned, setTotalEarned]   = useState(0);
  const [chartData, setChartData]       = useState<{ day: string; nmc: number }[]>([]);
  const [loading, setLoading]           = useState(true);
  const [error, setError]               = useState<string | null>(null);

  // Load account ID from device storage on mount
  useEffect(() => {
    if (typeof window !== "undefined") {
      setAccountId(localStorage.getItem("nm_account_id"));
    }
  }, []);

  // Fetch real data once we have an account ID
  useEffect(() => {
    if (!accountId) { setLoading(false); return; }
    setLoading(true);
    Promise.all([
      api.listJobs(accountId, 50),
      api.getBalance(accountId),
    ])
      .then(([{ jobs: list }, balance]) => {
        setJobs(list);
        setTotalEarned(balance.total_earned_nmc);

        // Build 14-day chart from real job data
        const days: Record<string, number> = {};
        const now = Date.now();
        for (let i = 13; i >= 0; i--) {
          const d = new Date(now - i * 86_400_000);
          days[d.toLocaleDateString("en-US", { month: "short", day: "numeric" })] = 0;
        }
        list.forEach((j) => {
          if (j.actual_cost_nmc && j.completed_at) {
            const key = new Date(j.completed_at).toLocaleDateString("en-US", {
              month: "short", day: "numeric",
            });
            if (key in days) days[key] += j.actual_cost_nmc * 0.92; // provider's 92%
          }
        });
        setChartData(Object.entries(days).map(([day, nmc]) => ({ day, nmc })));
      })
      .catch(() => setError("Could not reach coordinator. Is the backend running?"))
      .finally(() => setLoading(false));
  }, [accountId]);

  const runningJobs   = jobs.filter((j) => j.state === "running").length;
  const completedJobs = jobs.filter((j) => j.state === "complete").length;
  const totalCost     = jobs.reduce((s, j) => s + (j.actual_cost_nmc ?? 0), 0);

  // Not registered yet
  if (!loading && !accountId) {
    return (
      <div className="min-h-screen pt-14 flex items-center justify-center">
        <div className="text-center space-y-4 max-w-sm px-4">
          <AlertCircle className="h-10 w-10 text-yellow-400 mx-auto" />
          <h2 className="text-lg font-semibold text-white">No account found</h2>
          <p className="text-slate-400 text-sm">
            Create a device-linked account first, then register your Mac as a provider.
          </p>
          <a
            href="/account"
            className="inline-block px-5 py-2.5 rounded-lg bg-brand-400/10 border border-brand-400/20 text-brand-400 text-sm font-medium hover:bg-brand-400/20 transition-colors"
          >
            Go to Account →
          </a>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen pt-14">
      <div className="max-w-6xl mx-auto px-4 sm:px-6 py-8">

        {/* Header */}
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 mb-8">
          <div>
            <h1 className="text-2xl font-bold text-white">Provider Dashboard</h1>
            <p className="text-slate-400 text-sm mt-1">
              Monitor earnings, jobs, and agent status.
            </p>
          </div>
          <div className="flex items-center gap-3">
            <div className={cn(
              "flex items-center gap-2 px-3 py-1.5 rounded-lg border text-sm font-medium",
              agentRunning === true
                ? "border-green-400/30 bg-green-400/5 text-green-400"
                : agentRunning === false
                  ? "border-red-400/30 bg-red-400/5 text-red-400"
                  : "border-slate-700 text-slate-500"
            )}>
              <span className={cn(
                "h-2 w-2 rounded-full",
                agentRunning === true  ? "bg-green-400 animate-pulse" :
                agentRunning === false ? "bg-red-400" : "bg-slate-600"
              )} />
              {agentRunning === true  ? "Agent running" :
               agentRunning === false ? "Agent stopped" : "Agent unknown"}
            </div>
          </div>
        </div>

        {/* Error banner */}
        {error && (
          <div className="mb-6 flex items-center gap-3 px-4 py-3 rounded-xl border border-red-400/20 bg-red-400/5 text-red-400 text-sm">
            <AlertCircle className="h-4 w-4 flex-shrink-0" />
            {error}
          </div>
        )}

        {/* Stats row */}
        <div className="grid grid-cols-2 lg:grid-cols-4 gap-4 mb-8">
          <StatCard icon={DollarSign}  label="Total earned"  value={formatNmc(totalEarned)}   color="text-green-400"  />
          <StatCard icon={Activity}    label="Running jobs"  value={String(runningJobs)}       color="text-brand-400" />
          <StatCard icon={CheckCircle} label="Completed"     value={String(completedJobs)}     color="text-blue-400"  />
          <StatCard icon={TrendingUp}  label="This session"  value={formatNmc(totalCost, 3)}   color="text-purple-400"/>
        </div>

        {/* Earnings chart */}
        <div className="glass rounded-xl p-5 mb-8">
          <h2 className="text-sm font-semibold text-white mb-4">Earnings (last 14 days)</h2>
          {chartData.every((d) => d.nmc === 0) ? (
            <div className="h-40 flex items-center justify-center text-slate-600 text-sm">
              {loading ? "Loading earnings data…" : "No completed jobs in the last 14 days."}
            </div>
          ) : (
            <div className="h-40">
              <ResponsiveContainer width="100%" height="100%">
                <AreaChart data={chartData}>
                  <defs>
                    <linearGradient id="nmcGrad" x1="0" y1="0" x2="0" y2="1">
                      <stop offset="5%"  stopColor="#ffe566" stopOpacity={0.3} />
                      <stop offset="95%" stopColor="#ffe566" stopOpacity={0}   />
                    </linearGradient>
                  </defs>
                  <XAxis dataKey="day" tick={{ fill: "#64748b", fontSize: 10 }} axisLine={false} tickLine={false} />
                  <YAxis tick={{ fill: "#64748b", fontSize: 10 }} axisLine={false} tickLine={false} width={40}
                    tickFormatter={(v: number) => v.toFixed(2)} />
                  <Tooltip
                    contentStyle={{ background: "#0f172a", border: "1px solid #334155", borderRadius: 8, fontSize: 12 }}
                    labelStyle={{ color: "#94a3b8" }}
                    formatter={(v: number) => [`${v.toFixed(4)} HC`, "Earned"]}
                  />
                  <Area type="monotone" dataKey="nmc" stroke="#ffe566" strokeWidth={2} fill="url(#nmcGrad)" />
                </AreaChart>
              </ResponsiveContainer>
            </div>
          )}
        </div>

        {/* Setup guide (if no jobs yet) */}
        {jobs.length === 0 && !loading && !error && (
          <div className="glass rounded-xl p-6 mb-8 glow-border">
            <h2 className="text-lg font-semibold text-white mb-4">Set up your provider</h2>
            <p className="text-slate-400 text-sm mb-6">
              Run the installer on your Mac Mini or Mac Studio to start earning HC.
            </p>
            <div className="space-y-3">
              {[
                { label: "One-line install", cmd: "curl -fsSL https://raw.githubusercontent.com/wkang0223/neuralmesh/master/scripts/install-agent-macos.sh | bash" },
                { label: "Or via Homebrew",  cmd: "brew install hatch/tap/nm && nm provider install" },
                { label: "Start agent",      cmd: "nm provider start" },
              ].map((s) => (
                <div key={s.cmd} className="flex items-center gap-3 bg-slate-900 rounded-lg px-4 py-3 border border-slate-800">
                  <code className="flex-1 font-mono text-xs text-slate-300">{s.cmd}</code>
                  <button
                    onClick={() => { navigator.clipboard.writeText(s.cmd); toast.success("Copied!"); }}
                    className="text-slate-500 hover:text-brand-400 transition-colors"
                  >
                    <Copy className="h-3.5 w-3.5" />
                  </button>
                </div>
              ))}
            </div>
          </div>
        )}

        {/* Recent jobs */}
        <div className="glass rounded-xl overflow-hidden">
          <div className="px-5 py-4 border-b border-slate-800 flex items-center justify-between">
            <h2 className="text-sm font-semibold text-white">Recent Jobs</h2>
            <span className="text-xs text-slate-500">{jobs.length} jobs</span>
          </div>

          {loading ? (
            <div className="py-12 text-center text-slate-500 text-sm">Loading jobs…</div>
          ) : jobs.length === 0 ? (
            <div className="py-12 text-center text-slate-500 text-sm">
              No jobs yet. Jobs you run appear here.
            </div>
          ) : (
            <div className="divide-y divide-slate-800">
              {jobs.map((job) => (
                <JobRow key={job.id} job={job} />
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function StatCard({ icon: Icon, label, value, color }: {
  icon: React.ElementType;
  label: string;
  value: string;
  color: string;
}) {
  return (
    <div className="glass rounded-xl p-4">
      <Icon className={cn("h-4 w-4 mb-2", color)} />
      <div className={cn("text-xl font-bold font-mono", color)}>{value}</div>
      <div className="text-xs text-slate-500 mt-0.5">{label}</div>
    </div>
  );
}

function JobRow({ job }: { job: Job }) {
  const date = new Date(job.created_at).toLocaleString(undefined, {
    month: "short", day: "numeric", hour: "2-digit", minute: "2-digit",
  });

  return (
    <div className="px-5 py-3.5 flex items-center gap-4 hover:bg-slate-900/40 transition-colors">
      <div className="flex-shrink-0">
        {job.state === "complete"  && <CheckCircle className="h-4 w-4 text-blue-400" />}
        {job.state === "running"   && <Activity    className="h-4 w-4 text-green-400 animate-pulse" />}
        {job.state === "failed"    && <XCircle     className="h-4 w-4 text-red-400" />}
        {(job.state === "queued" || job.state === "matching") && <Clock className="h-4 w-4 text-yellow-400" />}
        {job.state === "cancelled" && <XCircle     className="h-4 w-4 text-slate-500" />}
      </div>

      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span className="font-mono text-xs text-slate-300 truncate">{job.id}</span>
          <span className="text-xs px-1.5 py-0.5 rounded bg-slate-800 text-slate-400 border border-slate-700">
            {runtimeShort(job.runtime)}
          </span>
        </div>
        <div className="text-xs text-slate-500 mt-0.5">{date}</div>
      </div>

      <div className="text-xs text-slate-400 hidden sm:block">{job.min_ram_gb} GB</div>

      <div className="text-xs font-mono text-right">
        {job.actual_cost_nmc !== undefined ? (
          <span className={job.actual_cost_nmc > 0 ? "text-green-400" : "text-slate-500"}>
            {formatNmc(job.actual_cost_nmc, 4)}
          </span>
        ) : (
          <span className="text-slate-500">—</span>
        )}
      </div>

      <span className={cn("text-xs px-2 py-0.5 rounded-full hidden md:block", stateColor(job.state))}>
        {job.state}
      </span>

      <a href={`/jobs/${job.id}`} className="text-slate-500 hover:text-brand-400 transition-colors">
        <Terminal className="h-3.5 w-3.5" />
      </a>
    </div>
  );
}
