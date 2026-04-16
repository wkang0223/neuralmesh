import Link from "next/link";
import {
  Cpu, Zap, Shield, Globe, ChevronRight,
  Terminal, DollarSign, Clock, TrendingUp,
  Lock, ArrowRight, ShieldCheck,
} from "lucide-react";
import NetworkStatsBar from "@/components/NetworkStatsBar";

// ── Static data for landing page ─────────────────────────────────────────────

const MAC_SPECS = [
  { model: "Mac Mini M4",       ram: "16–24 GB", gpu: "10-core", best: "Small inference, simulation",   price: "$0.03" },
  { model: "Mac Mini M4 Pro",   ram: "24–64 GB", gpu: "20-core", best: "30B LLM inference, SD",        price: "$0.07" },
  { model: "Mac Studio M4 Max", ram: "64–128 GB",gpu: "40-core", best: "70B LLM inference",             price: "$0.12" },
  { model: "Mac Studio M4 Ultra",ram: "128–192 GB",gpu: "80-core",best: "Full 405B model shards",       price: "$0.22" },
];

const FEATURES = [
  {
    icon: Cpu,
    title: "Apple Silicon Advantage",
    body: "Unified memory means a $1,400 Mac Mini M4 Pro runs Llama 3 70B — hardware that normally requires two $15,000 A100 cards.",
  },
  {
    icon: Shield,
    title: "Sandboxed Execution",
    body: "Every job runs in a dedicated macOS sandbox-exec profile: restricted filesystem, localhost-only network, isolated OS user.",
  },
  {
    icon: Zap,
    title: "Zero-Copy GPU Access",
    body: "MLX and PyTorch MPS use Metal directly. No CPU↔GPU copies, no driver headaches — full unified memory bandwidth from day one.",
  },
  {
    icon: Globe,
    title: "Decentralized Network",
    body: "libp2p Kademlia DHT — no single point of failure. Providers connect peer-to-peer; the coordinator cluster is just a matchmaker.",
  },
  {
    icon: DollarSign,
    title: "Earn While You Sleep",
    body: "Automatic idle detection via IOKit + CGSession. Your Mac starts accepting jobs only when screen is locked and GPU is idle.",
  },
  {
    icon: TrendingUp,
    title: "HC Credit System",
    body: "Off-chain HC credits for Phase 1. On-chain Arbitrum L2 token for trustless settlement in Phase 3 — no gas fees in the interim.",
  },
];

const STEPS_PROVIDER = [
  { step: "1", cmd: "curl -fsSL https://raw.githubusercontent.com/wkang0223/neuralmesh/master/scripts/install-agent-macos.sh | bash", label: "Install agent" },
  { step: "2", cmd: "nm provider config --idle-minutes 10 --floor-price 0.05", label: "Set your price" },
  { step: "3", cmd: "nm provider start", label: "Start earning" },
];

const STEPS_CONSUMER = [
  { step: "1", cmd: "brew install hatch/tap/nm", label: "Install CLI" },
  { step: "2", cmd: "nm gpu list --min-ram 48 --runtime mlx", label: "Browse GPUs" },
  { step: "3", cmd: "nm job submit --runtime mlx --ram 48 ./inference.py", label: "Run your job" },
];

// ── Page ──────────────────────────────────────────────────────────────────────

export default function LandingPage() {
  return (
    <div className="flex flex-col min-h-screen">

      {/* ── Hero ─────────────────────────────────────────────────────────── */}
      <section className="relative pt-28 pb-20 overflow-hidden">
        {/* Background grid + glow */}
        <div className="absolute inset-0 grid-bg opacity-40 pointer-events-none" />
        <div
          className="absolute inset-0 pointer-events-none"
          style={{
            background:
              "radial-gradient(ellipse 80% 50% at 50% -5%, rgba(255,229,102,0.15), transparent 70%)",
          }}
        />

        <div className="relative max-w-5xl mx-auto px-4 sm:px-6 text-center">
          {/* Badge */}
          <div className="inline-flex items-center gap-2 px-3 py-1 rounded-full border border-brand-400/30 bg-brand-400/5 text-brand-300 text-xs font-medium mb-6">
            <span className="h-1.5 w-1.5 rounded-full bg-brand-400 animate-pulse" />
            Phase 1 — Apple Silicon · Mainnet Preview
          </div>

          {/* Headline */}
          <h1 className="text-5xl sm:text-7xl font-extrabold tracking-tight text-white leading-tight mb-6">
            The World&apos;s{" "}
            <span className="gradient-text">Idle Mac GPUs</span>
            <br />
            Working For You
          </h1>

          <p className="text-lg sm:text-xl text-slate-400 max-w-2xl mx-auto mb-4 leading-relaxed">
            A decentralized marketplace for Apple M-series unified memory compute.
            Run Llama 3 70B on a Mac Mini M4 Pro for{" "}
            <span className="text-white font-semibold">$0.07/hr</span>.{" "}
            Earn HC credits while your Mac sleeps.
          </p>

          <p className="text-sm text-slate-500 max-w-xl mx-auto mb-10">
            Mac Mini M4 Pro (48 GB) = two A100 80GB cards ($30,000+) for AI inference.
            No CUDA required. No cloud markup. Just Metal.
          </p>

          {/* CTAs */}
          <div className="flex flex-col sm:flex-row gap-3 justify-center">
            <Link
              href="/market"
              className="inline-flex items-center gap-2 px-6 py-3 rounded-lg bg-brand-400 text-slate-950 font-semibold hover:bg-brand-300 transition-colors shadow-lg shadow-brand-400/20"
            >
              <Zap className="h-4 w-4" />
              Browse GPUs
              <ChevronRight className="h-4 w-4" />
            </Link>
            <Link
              href="#provider"
              className="inline-flex items-center gap-2 px-6 py-3 rounded-lg border border-slate-700 text-slate-300 font-semibold hover:bg-slate-800 hover:border-slate-600 transition-colors"
            >
              <DollarSign className="h-4 w-4" />
              Earn as Provider
            </Link>
          </div>

          {/* Live stats bar */}
          <div className="mt-14">
            <NetworkStatsBar />
          </div>
        </div>
      </section>

      {/* ── Mac Specs comparison ──────────────────────────────────────────── */}
      <section className="py-16 border-t border-slate-800/50">
        <div className="max-w-6xl mx-auto px-4 sm:px-6">
          <h2 className="text-2xl font-bold text-center text-white mb-2">
            Apple Silicon — The Compute Advantage
          </h2>
          <p className="text-center text-slate-500 mb-10 text-sm">
            Unified memory = GPU memory. No separate VRAM. A $1,400 Mac runs what needs $30,000 in NVIDIA cards.
          </p>

          <div className="overflow-x-auto rounded-xl border border-slate-800">
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-slate-800 bg-slate-900/50">
                  <th className="text-left px-4 py-3 text-slate-400 font-medium">Mac Model</th>
                  <th className="text-left px-4 py-3 text-slate-400 font-medium">Unified Memory</th>
                  <th className="text-left px-4 py-3 text-slate-400 font-medium">GPU Cores</th>
                  <th className="text-left px-4 py-3 text-slate-400 font-medium">Best For</th>
                  <th className="text-right px-4 py-3 text-slate-400 font-medium">Est. Price</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-800">
                {MAC_SPECS.map((mac, i) => (
                  <tr
                    key={mac.model}
                    className="hover:bg-slate-900/40 transition-colors"
                  >
                    <td className={`px-4 py-3 font-medium ${i === 1 ? "text-brand-300" : "text-white"}`}>
                      {mac.model}
                      {i === 1 && (
                        <span className="ml-2 text-xs px-1.5 py-0.5 rounded bg-brand-400/10 text-brand-400 border border-brand-400/20">
                          Popular
                        </span>
                      )}
                    </td>
                    <td className="px-4 py-3 text-slate-300 font-mono">{mac.ram}</td>
                    <td className="px-4 py-3 text-slate-300">{mac.gpu}</td>
                    <td className="px-4 py-3 text-slate-400">{mac.best}</td>
                    <td className="px-4 py-3 text-right text-green-400 font-mono font-semibold">
                      {mac.price}/hr
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          <p className="text-center text-xs text-slate-600 mt-3">
            Compared to AWS p4d.24xlarge (8× A100): $32.77/hr · No Apple Silicon support · 3× higher latency for LLM inference
          </p>
        </div>
      </section>

      {/* ── Features ─────────────────────────────────────────────────────── */}
      <section className="py-16 border-t border-slate-800/50">
        <div className="max-w-6xl mx-auto px-4 sm:px-6">
          <h2 className="text-2xl font-bold text-center text-white mb-10">
            Built for Real GPU Compute
          </h2>
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-6">
            {FEATURES.map((f) => (
              <div
                key={f.title}
                className="glass rounded-xl p-5 hover:border-slate-700 transition-colors"
              >
                <f.icon className="h-5 w-5 text-brand-400 mb-3" />
                <h3 className="font-semibold text-white mb-1">{f.title}</h3>
                <p className="text-sm text-slate-400 leading-relaxed">{f.body}</p>
              </div>
            ))}
          </div>
        </div>
      </section>

      {/* ── Get Started: Consumer ─────────────────────────────────────────── */}
      <section className="py-16 border-t border-slate-800/50">
        <div className="max-w-4xl mx-auto px-4 sm:px-6">
          <div className="grid md:grid-cols-2 gap-12">

            {/* Consumer */}
            <div>
              <div className="inline-flex items-center gap-2 text-xs font-medium text-brand-400 bg-brand-400/10 border border-brand-400/20 px-3 py-1 rounded-full mb-4">
                <Zap className="h-3 w-3" /> For Consumers
              </div>
              <h2 className="text-2xl font-bold text-white mb-2">Run AI jobs in 3 steps</h2>
              <p className="text-slate-400 text-sm mb-6">
                Submit Python scripts to the network. MLX, PyTorch MPS, ONNX CoreML — choose your runtime.
              </p>
              <div className="space-y-3">
                {STEPS_CONSUMER.map((s) => (
                  <div key={s.step} className="flex gap-3">
                    <div className="flex-shrink-0 h-6 w-6 rounded-full bg-brand-400/10 border border-brand-400/30 text-brand-400 text-xs font-bold flex items-center justify-center">
                      {s.step}
                    </div>
                    <div>
                      <div className="text-xs text-slate-500 mb-0.5">{s.label}</div>
                      <code className="text-xs font-mono text-slate-300 bg-slate-900 rounded px-2 py-1 block border border-slate-800">
                        {s.cmd}
                      </code>
                    </div>
                  </div>
                ))}
              </div>
              <Link
                href="/market"
                className="inline-flex items-center gap-2 mt-6 text-sm text-brand-400 hover:text-brand-300 font-medium"
              >
                Browse available GPUs <ChevronRight className="h-3.5 w-3.5" />
              </Link>
            </div>

            {/* Provider */}
            <div id="provider">
              <div className="inline-flex items-center gap-2 text-xs font-medium text-green-400 bg-green-400/10 border border-green-400/20 px-3 py-1 rounded-full mb-4">
                <DollarSign className="h-3 w-3" /> For Providers
              </div>
              <h2 className="text-2xl font-bold text-white mb-2">Earn while your Mac sleeps</h2>
              <p className="text-slate-400 text-sm mb-6">
                Install the agent on your Mac Mini or Mac Studio. Idle detection is automatic — jobs only run when your screen is locked.
              </p>
              <div className="space-y-3">
                {STEPS_PROVIDER.map((s) => (
                  <div key={s.step} className="flex gap-3">
                    <div className="flex-shrink-0 h-6 w-6 rounded-full bg-green-400/10 border border-green-400/30 text-green-400 text-xs font-bold flex items-center justify-center">
                      {s.step}
                    </div>
                    <div>
                      <div className="text-xs text-slate-500 mb-0.5">{s.label}</div>
                      <code className="text-xs font-mono text-slate-300 bg-slate-900 rounded px-2 py-1 block border border-slate-800">
                        {s.cmd}
                      </code>
                    </div>
                  </div>
                ))}
              </div>
              <Link
                href="/provider"
                className="inline-flex items-center gap-2 mt-6 text-sm text-green-400 hover:text-green-300 font-medium"
              >
                Provider dashboard <ChevronRight className="h-3.5 w-3.5" />
              </Link>
            </div>

          </div>
        </div>
      </section>

      {/* ── Python SDK install ────────────────────────────────────────────── */}
      <section className="py-16 border-t border-slate-800/50">
        <div className="max-w-4xl mx-auto px-4 sm:px-6">
          <h2 className="text-2xl font-bold text-center text-white mb-2">
            Use from Python or CLI
          </h2>
          <p className="text-center text-slate-500 text-sm mb-8">
            One command to install — works with any Python project or Jupyter notebook.
          </p>

          <div className="grid md:grid-cols-2 gap-6">
            <div className="glass rounded-xl p-5">
              <div className="flex items-center gap-2 mb-3">
                <Terminal className="h-4 w-4 text-brand-400" />
                <span className="text-sm font-medium text-white">Python SDK</span>
                <span className="ml-auto text-xs text-slate-500 font-mono">pip</span>
              </div>
              <pre className="terminal text-xs leading-relaxed">{`pip install hatch

import hatch as nm

nm.configure(account_id="your-id")

job = nm.submit(
    script="./inference.py",
    runtime="mlx",
    ram_gb=48,
    hours=2,
)

for line in job.stream_logs():
    print(line, end="")`}</pre>
            </div>

            <div className="glass rounded-xl p-5">
              <div className="flex items-center gap-2 mb-3">
                <Terminal className="h-4 w-4 text-green-400" />
                <span className="text-sm font-medium text-white">CLI (Homebrew)</span>
                <span className="ml-auto text-xs text-slate-500 font-mono">brew</span>
              </div>
              <pre className="terminal text-xs leading-relaxed">{`# Install
brew install hatch/tap/nm

# Browse available Macs
nm gpu list --min-ram 48 --runtime mlx

# Submit a job
nm job submit \\
  --runtime mlx \\
  --ram 48 \\
  --hours 2 \\
  ./llama_inference.py

# Stream logs
nm job logs <job-id> --follow`}</pre>
            </div>
          </div>
        </div>
      </section>

      {/* ── How Credits Work ─────────────────────────────────────────────── */}
      <section className="py-16 border-t border-slate-800/50">
        <div className="max-w-5xl mx-auto px-4 sm:px-6">
          <div className="text-center mb-10">
            <h2 className="text-2xl font-bold text-white mb-2">How Credits &amp; Payments Work</h2>
            <p className="text-slate-400 text-sm max-w-xl mx-auto">
              Transparent, real-time accounting. Every HC is tracked — no hidden fees, no surprises.
            </p>
          </div>

          {/* Flow steps */}
          <div className="grid sm:grid-cols-5 gap-2 items-start mb-10">
            {[
              {
                icon: DollarSign, color: "text-green-400", bg: "bg-green-500/10 border-green-500/20",
                step: "1", label: "Deposit", sub: "$1 = 1 HC",
                body: "Pay via Stripe or wire HC from Solana / Arbitrum. Credits appear instantly.",
              },
              { arrow: true },
              {
                icon: Lock, color: "text-yellow-400", bg: "bg-yellow-500/10 border-yellow-500/20",
                step: "2", label: "Escrow", sub: "max budget held",
                body: "When a job is matched, your max budget is moved to escrow — safe, not spent.",
              },
              { arrow: true },
              {
                icon: Cpu, color: "text-brand-400", bg: "bg-brand-400/10 border-brand-400/20",
                step: "3", label: "GPU Runs", sub: "price/hr × time",
                body: "The provider's GPU executes your job. Metered to the second.",
              },
              { arrow: true },
              {
                icon: ArrowRight, color: "text-violet-400", bg: "bg-violet-500/10 border-violet-500/20",
                step: "4", label: "Settlement", sub: "auto on complete",
                body: "92% to provider · 8% platform fee · unused budget refunded to you.",
              },
              { arrow: true },
              {
                icon: TrendingUp, color: "text-slate-300", bg: "bg-slate-700/30 border-slate-600/30",
                step: "5", label: "Withdraw", sub: "anytime",
                body: "Providers cash out to their Solana or Arbitrum wallet. No lock-up.",
              },
            ].map((item, i) =>
              "arrow" in item ? (
                <div key={i} className="hidden sm:flex items-start justify-center pt-5">
                  <ChevronRight className="h-5 w-5 text-slate-700" />
                </div>
              ) : (
                <div key={i} className="glass rounded-xl p-4 flex flex-col items-center text-center gap-2">
                  <div className={`h-9 w-9 rounded-lg flex items-center justify-center border ${item.bg}`}>
                    <item.icon className={`h-4 w-4 ${item.color}`} />
                  </div>
                  <div className={`text-xs font-mono font-bold ${item.color}`}>{item.sub}</div>
                  <div className="text-sm font-semibold text-white">{item.label}</div>
                  <p className="text-xs text-slate-500 leading-relaxed">{item.body}</p>
                </div>
              )
            )}
          </div>

          {/* Money split diagram */}
          <div className="glass rounded-2xl p-6 md:p-8 max-w-2xl mx-auto">
            <div className="text-center text-sm font-semibold text-white mb-6">
              Example: 4-hour job at 0.10 HC/hr = <span className="text-brand-400">0.40 HC total</span>
            </div>

            {/* Bar */}
            <div className="flex rounded-full overflow-hidden h-5 mb-4 gap-px">
              <div className="bg-green-400" style={{ width: "92%" }} title="Provider (92%)" />
              <div className="bg-slate-600" style={{ width: "8%" }} title="Platform fee (8%)" />
            </div>
            <div className="flex justify-between text-xs mb-6">
              <div className="flex items-center gap-1.5">
                <div className="h-2.5 w-2.5 rounded-full bg-green-400" />
                <span className="text-green-400 font-mono font-semibold">0.368 HC</span>
                <span className="text-slate-500">provider (92%)</span>
              </div>
              <div className="flex items-center gap-1.5">
                <div className="h-2.5 w-2.5 rounded-full bg-slate-500" />
                <span className="text-slate-400 font-mono font-semibold">0.032 HC</span>
                <span className="text-slate-500">platform (8%)</span>
              </div>
            </div>

            {/* Refund note */}
            <div className="flex items-center gap-3 p-3 rounded-xl bg-slate-900/60 text-xs text-slate-400">
              <ShieldCheck className="h-4 w-4 text-yellow-400 flex-shrink-0" />
              If the job finishes early (e.g. 2 hrs instead of 4), you only pay for 2 hrs.
              The remaining 0.20 HC is automatically refunded from escrow.
            </div>
          </div>

          {/* Phase note */}
          <div className="mt-8 flex flex-wrap justify-center gap-4 text-xs text-slate-500">
            <div className="flex items-center gap-1.5">
              <span className="h-2 w-2 rounded-full bg-brand-400" />
              <strong className="text-slate-400">Phase 1 (now):</strong> Off-chain HC ledger (PostgreSQL)
            </div>
            <div className="flex items-center gap-1.5">
              <span className="h-2 w-2 rounded-full bg-violet-400" />
              <strong className="text-slate-400">Phase 2:</strong> On-chain SPL token (Solana) + ERC-20 (Arbitrum)
            </div>
            <div className="flex items-center gap-1.5">
              <span className="h-2 w-2 rounded-full bg-slate-500" />
              <strong className="text-slate-400">Phase 3:</strong> ZK compute proofs — trustless settlement
            </div>
          </div>
        </div>
      </section>

      {/* ── Roadmap ───────────────────────────────────────────────────────── */}
      <section className="py-16 border-t border-slate-800/50">
        <div className="max-w-4xl mx-auto px-4 sm:px-6">
          <h2 className="text-2xl font-bold text-center text-white mb-10">Roadmap</h2>
          <div className="relative">
            <div className="absolute left-4 top-0 bottom-0 w-px bg-slate-800" />
            <div className="space-y-6 pl-10">
              {[
                { phase: "Phase 1", label: "Apple Silicon MVP", active: true,
                  items: ["Mac Mini M4 / Mac Studio providers", "MLX · PyTorch MPS · ONNX CoreML", "Off-chain HC credits", "macOS sandbox isolation"] },
                { phase: "Phase 2", label: "Hardening + Migration", active: false,
                  items: ["DMTCP job checkpointing + migration", "macOS Virtualization.framework", "Reputation system", "Next.js dashboard v1"] },
                { phase: "Phase 3", label: "On-chain Token", active: false,
                  items: ["HC ERC-20 on Arbitrum L2", "Trustless escrow smart contracts", "ZK Groth16 compute proofs", "Provider staking"] },
                { phase: "Phase 4", label: "NVIDIA + AMD + Intel Arc", active: false,
                  items: ["Linux agent (nvml · rocm_smi · Level Zero)", "Docker + NVIDIA Container Toolkit", "Firecracker MicroVMs", "Apache TVM cross-platform compiler"] },
              ].map((phase) => (
                <div key={phase.phase} className="relative">
                  <div className={`absolute -left-6 h-3 w-3 rounded-full border-2 ${
                    phase.active
                      ? "bg-brand-400 border-brand-400 shadow-[0_0_8px_rgba(255,229,102,0.6)]"
                      : "bg-slate-800 border-slate-600"
                  }`} />
                  <div className={`glass rounded-xl p-4 ${phase.active ? "glow-border" : ""}`}>
                    <div className="flex items-center gap-2 mb-2">
                      <span className={`text-xs font-mono px-2 py-0.5 rounded ${
                        phase.active ? "bg-brand-400/10 text-brand-400" : "bg-slate-800 text-slate-500"
                      }`}>{phase.phase}</span>
                      <span className={`font-semibold text-sm ${phase.active ? "text-white" : "text-slate-400"}`}>
                        {phase.label}
                      </span>
                      {phase.active && (
                        <span className="ml-auto text-xs text-brand-400 flex items-center gap-1">
                          <span className="h-1.5 w-1.5 rounded-full bg-brand-400 animate-pulse" />
                          In Progress
                        </span>
                      )}
                    </div>
                    <div className="flex flex-wrap gap-2">
                      {phase.items.map((item) => (
                        <span key={item} className="text-xs text-slate-400 bg-slate-900 border border-slate-800 px-2 py-0.5 rounded">
                          {item}
                        </span>
                      ))}
                    </div>
                  </div>
                </div>
              ))}
            </div>
          </div>
        </div>
      </section>

      {/* ── Footer ────────────────────────────────────────────────────────── */}
      <footer className="border-t border-slate-800 py-8 mt-auto">
        <div className="max-w-6xl mx-auto px-4 sm:px-6 flex flex-col sm:flex-row items-center justify-between gap-4">
          <div className="flex items-center gap-2 text-slate-500 text-sm">
            <Cpu className="h-4 w-4 text-brand-400" />
            <span>Hatch · Apache 2.0</span>
          </div>
          <div className="flex items-center gap-6 text-sm text-slate-500">
            <Link href="/market" className="hover:text-white transition-colors">Market</Link>
            <Link href="/provider" className="hover:text-white transition-colors">Provider</Link>
            <a href="https://github.com/wkang0223/neuralmesh" className="hover:text-white transition-colors" target="_blank" rel="noreferrer">
              GitHub
            </a>
            <a href="https://docs.hatch.dev" className="hover:text-white transition-colors" target="_blank" rel="noreferrer">
              Docs
            </a>
          </div>
        </div>
      </footer>

    </div>
  );
}
