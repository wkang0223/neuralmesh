"use client";

import { useEffect, useState, useCallback } from "react";
import {
  createIdentity, loadIdentity, verifyDevice, deleteIdentity,
  reRegisterDevice, type DeviceIdentity,
} from "@/lib/device-id";
import { cn } from "@/lib/utils";
import {
  ShieldCheck, ShieldAlert, Cpu, Laptop, Fingerprint,
  KeyRound, Trash2, RefreshCw, Copy, CheckCheck, Info,
  ArrowRight, DollarSign, Lock, Zap, TrendingUp, ChevronDown, ChevronUp,
} from "lucide-react";
import { toast } from "sonner";

// ─── Types ────────────────────────────────────────────────────────────────────

type AccountStatus = "checking" | "none" | "verified" | "mismatch" | "unverified";

// ─── Money flow step card ─────────────────────────────────────────────────────

function FlowStep({
  icon: Icon,
  color,
  title,
  body,
  amount,
  arrow = true,
}: {
  icon: React.ElementType;
  color: string;
  title: string;
  body: string;
  amount?: string;
  arrow?: boolean;
}) {
  return (
    <div className="flex flex-col items-center gap-1 flex-1 min-w-[120px]">
      <div className={cn(
        "h-10 w-10 rounded-xl flex items-center justify-center border",
        color
      )}>
        <Icon className="h-5 w-5" />
      </div>
      <div className="text-xs font-semibold text-white text-center mt-1">{title}</div>
      {amount && <div className="text-xs font-mono text-brand-400">{amount}</div>}
      <div className="text-xs text-slate-500 text-center leading-tight">{body}</div>
      {arrow && (
        <div className="hidden sm:flex absolute right-0 top-4 text-slate-600 text-lg translate-x-1/2">
          →
        </div>
      )}
    </div>
  );
}

// ─── Security badge ───────────────────────────────────────────────────────────

function StatusBadge({ status }: { status: AccountStatus }) {
  if (status === "checking") {
    return (
      <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs bg-slate-700/40 border border-slate-600/30 text-slate-400">
        <RefreshCw className="h-3 w-3 animate-spin" /> Checking…
      </span>
    );
  }
  if (status === "verified") {
    return (
      <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs bg-green-500/10 border border-green-500/30 text-green-400">
        <ShieldCheck className="h-3 w-3" /> Device verified
      </span>
    );
  }
  if (status === "mismatch") {
    return (
      <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs bg-red-500/10 border border-red-500/30 text-red-400">
        <ShieldAlert className="h-3 w-3" /> Device mismatch
      </span>
    );
  }
  if (status === "unverified") {
    return (
      <span className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs bg-yellow-500/10 border border-yellow-500/30 text-yellow-400">
        <ShieldAlert className="h-3 w-3" /> Unverified (offline)
      </span>
    );
  }
  return null;
}

// ─── Main page ────────────────────────────────────────────────────────────────

export default function AccountPage() {
  const [identity, setIdentity]   = useState<DeviceIdentity | null>(null);
  const [status, setStatus]       = useState<AccountStatus>("checking");
  const [deviceLabel, setDeviceLabel] = useState("");
  const [creating, setCreating]     = useState(false);
  const [reregistering, setReregistering] = useState(false);
  const [copied, setCopied]         = useState(false);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [showFlow, setShowFlow]   = useState(true);

  const check = useCallback(async () => {
    setStatus("checking");
    const id = await loadIdentity();
    if (!id) { setIdentity(null); setStatus("none"); return; }
    setIdentity(id);
    const result = await verifyDevice(id);
    setStatus(result.verified ? "verified" : "mismatch");
  }, []);

  useEffect(() => { check(); }, [check]);

  async function handleCreate() {
    setCreating(true);
    try {
      const label = deviceLabel.trim() || undefined;
      const id = await createIdentity(label);
      setIdentity(id);
      setStatus("verified");
      toast.success("Account created and locked to this device");
    } catch (e) {
      toast.error("Failed to create account: " + String(e));
    } finally {
      setCreating(false);
    }
  }

  async function handleReregister() {
    if (!identity) return;
    setReregistering(true);
    try {
      const updated = await reRegisterDevice(identity);
      setIdentity(updated);
      setStatus("verified");
      toast.success("Device profile updated and re-verified");
    } catch (e) {
      toast.error("Re-registration failed: " + String(e));
    } finally {
      setReregistering(false);
    }
  }

  async function handleDelete() {
    if (!confirm("Delete your local account identity? You cannot recover it if you haven't backed up your account ID.")) return;
    await deleteIdentity();
    setIdentity(null);
    setStatus("none");
    toast("Local identity cleared");
  }

  function copyAccountId() {
    if (!identity) return;
    navigator.clipboard.writeText(identity.accountId);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }

  const isLocked = status === "verified";

  return (
    <div className="min-h-screen pt-14">
      <div className="max-w-3xl mx-auto px-4 sm:px-6 py-8 space-y-8">

        {/* Header */}
        <div>
          <h1 className="text-2xl font-bold text-white">Account</h1>
          <p className="text-sm text-slate-500 mt-1">
            Your account is cryptographically locked to this device. It cannot be used from another computer.
          </p>
        </div>

        {/* ── How Credits Work ─────────────────────────────────────── */}
        <div className="glass rounded-xl overflow-hidden">
          <button
            onClick={() => setShowFlow(!showFlow)}
            className="w-full px-5 py-4 flex items-center justify-between text-left border-b border-slate-800"
          >
            <span className="text-sm font-semibold text-white flex items-center gap-2">
              <DollarSign className="h-4 w-4 text-brand-400" />
              How Credits &amp; Payments Work
            </span>
            {showFlow ? <ChevronUp className="h-4 w-4 text-slate-500" /> : <ChevronDown className="h-4 w-4 text-slate-500" />}
          </button>

          {showFlow && (
            <div className="px-5 py-6 space-y-6">
              {/* Flow diagram */}
              <div className="relative flex flex-wrap sm:flex-nowrap gap-4 justify-between">
                <div className="relative flex-1 min-w-[110px] flex flex-col items-center gap-1">
                  <div className="h-10 w-10 rounded-xl flex items-center justify-center border bg-green-500/10 border-green-500/20">
                    <DollarSign className="h-5 w-5 text-green-400" />
                  </div>
                  <div className="text-xs font-semibold text-white text-center mt-1">Deposit</div>
                  <div className="text-xs font-mono text-green-400">$1 = 1 HC</div>
                  <div className="text-xs text-slate-500 text-center leading-tight">Stripe → HC credits in your ledger</div>
                </div>
                <div className="hidden sm:flex items-center text-slate-600 text-xl mt-3">→</div>
                <div className="relative flex-1 min-w-[110px] flex flex-col items-center gap-1">
                  <div className="h-10 w-10 rounded-xl flex items-center justify-center border bg-yellow-500/10 border-yellow-500/20">
                    <Lock className="h-5 w-5 text-yellow-400" />
                  </div>
                  <div className="text-xs font-semibold text-white text-center mt-1">Escrow</div>
                  <div className="text-xs font-mono text-yellow-400">max budget locked</div>
                  <div className="text-xs text-slate-500 text-center leading-tight">Held safely while job runs</div>
                </div>
                <div className="hidden sm:flex items-center text-slate-600 text-xl mt-3">→</div>
                <div className="relative flex-1 min-w-[110px] flex flex-col items-center gap-1">
                  <div className="h-10 w-10 rounded-xl flex items-center justify-center border bg-brand-400/10 border-brand-400/20">
                    <Cpu className="h-5 w-5 text-brand-400" />
                  </div>
                  <div className="text-xs font-semibold text-white text-center mt-1">Job Runs</div>
                  <div className="text-xs font-mono text-brand-400">price/hour × time</div>
                  <div className="text-xs text-slate-500 text-center leading-tight">GPU compute on provider's Mac</div>
                </div>
                <div className="hidden sm:flex items-center text-slate-600 text-xl mt-3">→</div>
                <div className="relative flex-1 min-w-[110px] flex flex-col items-center gap-1">
                  <div className="h-10 w-10 rounded-xl flex items-center justify-center border bg-violet-500/10 border-violet-500/20">
                    <Zap className="h-5 w-5 text-violet-400" />
                  </div>
                  <div className="text-xs font-semibold text-white text-center mt-1">Settlement</div>
                  <div className="text-xs font-mono text-violet-400">automatic</div>
                  <div className="text-xs text-slate-500 text-center leading-tight">Escrow released on completion</div>
                </div>
              </div>

              {/* Settlement breakdown */}
              <div className="bg-slate-900/60 rounded-xl p-4 space-y-3">
                <div className="text-xs font-semibold text-slate-400 uppercase tracking-wider mb-3">Where Your HC Goes (example: 10 HC job)</div>

                <div className="space-y-2">
                  <div className="flex items-center gap-3">
                    <div className="w-2 h-2 rounded-full bg-brand-400 flex-shrink-0" />
                    <div className="flex-1 text-xs text-slate-300">You pay (actual compute time × price/hour)</div>
                    <div className="font-mono text-xs text-brand-400">10.000 HC</div>
                  </div>
                  <div className="ml-5 space-y-1.5 border-l border-slate-700 pl-4">
                    <div className="flex items-center gap-3">
                      <div className="w-1.5 h-1.5 rounded-full bg-green-400 flex-shrink-0" />
                      <div className="flex-1 text-xs text-slate-400">Provider earns (92%)</div>
                      <div className="font-mono text-xs text-green-400">9.200 HC</div>
                    </div>
                    <div className="flex items-center gap-3">
                      <div className="w-1.5 h-1.5 rounded-full bg-slate-500 flex-shrink-0" />
                      <div className="flex-1 text-xs text-slate-400">Hatch platform fee (8%)</div>
                      <div className="font-mono text-xs text-slate-400">0.800 HC</div>
                    </div>
                  </div>
                  <div className="flex items-center gap-3 pt-1">
                    <div className="w-2 h-2 rounded-full bg-yellow-400 flex-shrink-0" />
                    <div className="flex-1 text-xs text-slate-300">Unused budget refunded to you</div>
                    <div className="font-mono text-xs text-yellow-400">varies</div>
                  </div>
                </div>
              </div>

              {/* Provider payout flow */}
              <div className="bg-slate-900/60 rounded-xl p-4">
                <div className="text-xs font-semibold text-slate-400 uppercase tracking-wider mb-3">Provider payout flow</div>
                <div className="flex items-center gap-2 flex-wrap text-xs">
                  {[
                    { label: "Job completes", color: "text-slate-300" },
                    { label: "→" },
                    { label: "Escrow released", color: "text-brand-400" },
                    { label: "→" },
                    { label: "92% credited to provider HC balance", color: "text-green-400" },
                    { label: "→" },
                    { label: "Withdraw to Solana or Arbitrum wallet", color: "text-violet-400" },
                  ].map((item, i) =>
                    item.label === "→"
                      ? <span key={i} className="text-slate-600">→</span>
                      : <span key={i} className={cn("font-medium", item.color)}>{item.label}</span>
                  )}
                </div>
              </div>

              <div className="flex items-start gap-2 p-3 rounded-lg bg-blue-500/5 border border-blue-500/10">
                <Info className="h-4 w-4 text-blue-400 flex-shrink-0 mt-0.5" />
                <p className="text-xs text-slate-400 leading-relaxed">
                  <strong className="text-slate-300">Phase 1 (now):</strong> Credits are off-chain HC tracked in a PostgreSQL ledger.
                  You can withdraw to ETH (Arbitrum) or Solana at any time.{" "}
                  <strong className="text-slate-300">Phase 2:</strong> Credits become fully on-chain SPL tokens (Solana) or ERC-20
                  (Arbitrum) — no trust required.
                </p>
              </div>
            </div>
          )}
        </div>

        {/* ── Device identity ───────────────────────────────────────── */}
        <div className="glass rounded-xl overflow-hidden">
          <div className="px-5 py-4 border-b border-slate-800 flex items-center justify-between">
            <span className="text-sm font-semibold text-white flex items-center gap-2">
              <Fingerprint className="h-4 w-4 text-violet-400" />
              Device Identity
            </span>
            <StatusBadge status={status} />
          </div>

          <div className="p-5">
            {status === "checking" && (
              <div className="py-8 text-center text-slate-500 text-sm">
                <RefreshCw className="h-5 w-5 animate-spin mx-auto mb-2 text-slate-600" />
                Checking device identity…
              </div>
            )}

            {status === "none" && (
              <div className="space-y-5">
                <div className="text-center py-4">
                  <Laptop className="h-10 w-10 text-slate-600 mx-auto mb-3" />
                  <p className="text-slate-400 text-sm font-medium">No account on this device</p>
                  <p className="text-slate-500 text-xs mt-1 max-w-xs mx-auto">
                    Create an account to start submitting jobs. Your account will be
                    cryptographically locked to this computer — it can&apos;t be used from another device.
                  </p>
                </div>

                <div>
                  <label className="text-xs text-slate-500 block mb-1.5">
                    Device name <span className="text-slate-600">(optional)</span>
                  </label>
                  <input
                    type="text"
                    placeholder={`e.g. "MacBook Pro M4 Max"`}
                    value={deviceLabel}
                    onChange={(e) => setDeviceLabel(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && handleCreate()}
                    className="w-full px-3 py-2 rounded-lg bg-slate-900 border border-slate-700 text-sm text-slate-300 placeholder-slate-600 focus:outline-none focus:border-brand-400/50"
                  />
                </div>

                <button
                  onClick={handleCreate}
                  disabled={creating}
                  className="w-full py-3 rounded-xl bg-brand-400/10 border border-brand-400/30 text-brand-400 text-sm font-semibold hover:bg-brand-400/20 transition-colors disabled:opacity-50 flex items-center justify-center gap-2"
                >
                  {creating ? (
                    <><RefreshCw className="h-4 w-4 animate-spin" /> Generating keypair…</>
                  ) : (
                    <><KeyRound className="h-4 w-4" /> Create Device-Locked Account</>
                  )}
                </button>

                <div className="flex items-start gap-2 p-3 rounded-lg bg-slate-900/60 border border-slate-800">
                  <ShieldCheck className="h-4 w-4 text-violet-400 flex-shrink-0 mt-0.5" />
                  <p className="text-xs text-slate-400 leading-relaxed">
                    An ECDSA P-256 keypair is generated in your browser&apos;s secure storage.
                    The private key never leaves this device. Your account ID is derived
                    from the public key + a hardware fingerprint of this machine.
                  </p>
                </div>
              </div>
            )}

            {(status === "verified" || status === "unverified" || status === "mismatch") && identity && (
              <div className="space-y-4">

                {/* Identity card */}
                <div className="bg-slate-900/60 rounded-xl p-4 space-y-3">
                  <div className="flex items-center gap-3">
                    <div className="h-10 w-10 rounded-xl bg-gradient-to-br from-brand-400/20 to-violet-500/20 border border-brand-400/20 flex items-center justify-center flex-shrink-0">
                      <Laptop className="h-5 w-5 text-brand-400" />
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className="text-sm font-semibold text-white truncate">
                        {identity.deviceLabel}
                      </div>
                      <div className="text-xs text-slate-500">{identity.platform} · created {new Date(identity.createdAt).toLocaleDateString()}</div>
                    </div>
                    {isLocked && (
                      <ShieldCheck className="h-5 w-5 text-green-400 flex-shrink-0" />
                    )}
                  </div>

                  {/* Account ID */}
                  <div>
                    <div className="text-xs text-slate-500 mb-1">Account ID</div>
                    <div className="flex items-center gap-2 bg-slate-950/60 rounded-lg px-3 py-2">
                      <code className="text-xs font-mono text-brand-400 flex-1 break-all">
                        {identity.accountId}
                      </code>
                      <button
                        onClick={copyAccountId}
                        className="text-slate-500 hover:text-slate-300 transition-colors flex-shrink-0"
                      >
                        {copied
                          ? <CheckCheck className="h-4 w-4 text-green-400" />
                          : <Copy className="h-4 w-4" />}
                      </button>
                    </div>
                  </div>
                </div>

                {/* Mismatch — show re-register option */}
                {status === "mismatch" && (
                  <div className="rounded-lg bg-yellow-500/5 border border-yellow-500/20 p-3 space-y-2">
                    <div className="flex items-start gap-2">
                      <ShieldAlert className="h-4 w-4 text-yellow-400 flex-shrink-0 mt-0.5" />
                      <div className="text-xs text-yellow-300 leading-relaxed">
                        <strong>Device profile changed.</strong> Your browser environment has
                        changed since this account was created (e.g. OS update, screen
                        configuration, or browser reset). If this is still your device, update
                        the profile to re-verify.
                      </div>
                    </div>
                    <button
                      onClick={handleReregister}
                      disabled={reregistering}
                      className="w-full py-2 rounded-lg bg-yellow-400/10 border border-yellow-400/20 text-yellow-300 text-xs font-medium hover:bg-yellow-400/20 transition-colors flex items-center justify-center gap-1.5 disabled:opacity-50"
                    >
                      {reregistering
                        ? <><RefreshCw className="h-3.5 w-3.5 animate-spin" /> Updating…</>
                        : <><RefreshCw className="h-3.5 w-3.5" /> Update device profile</>}
                    </button>
                  </div>
                )}

                {/* Advanced info */}
                <button
                  onClick={() => setShowAdvanced(!showAdvanced)}
                  className="text-xs text-slate-500 hover:text-slate-400 flex items-center gap-1 transition-colors"
                >
                  {showAdvanced ? <ChevronUp className="h-3.5 w-3.5" /> : <ChevronDown className="h-3.5 w-3.5" />}
                  Technical details
                </button>

                {showAdvanced && (
                  <div className="bg-slate-950/60 rounded-xl p-4 space-y-2 text-xs font-mono">
                    <div className="flex gap-2">
                      <span className="text-slate-600 w-28 flex-shrink-0">pubkey (SPKI)</span>
                      <span className="text-slate-400 break-all">{identity.ecdsaPubkeyHex.slice(0, 48)}…</span>
                    </div>
                    <div className="flex gap-2">
                      <span className="text-slate-600 w-28 flex-shrink-0">fingerprint</span>
                      <span className="text-slate-400 break-all">{identity.deviceFingerprintHash.slice(0, 32)}…</span>
                    </div>
                    <div className="flex gap-2">
                      <span className="text-slate-600 w-28 flex-shrink-0">algorithm</span>
                      <span className="text-slate-400">ECDSA P-256 / SHA-256</span>
                    </div>
                    <div className="flex gap-2">
                      <span className="text-slate-600 w-28 flex-shrink-0">storage</span>
                      <span className="text-slate-400">IndexedDB (non-extractable)</span>
                    </div>
                  </div>
                )}

                {/* Actions */}
                <div className="flex gap-3">
                  <button
                    onClick={check}
                    className="flex-1 py-2 rounded-lg bg-slate-800 border border-slate-700 text-slate-300 text-xs font-medium hover:bg-slate-700 transition-colors flex items-center justify-center gap-1.5"
                  >
                    <RefreshCw className="h-3.5 w-3.5" /> Re-verify
                  </button>
                  <button
                    onClick={handleDelete}
                    className="py-2 px-4 rounded-lg bg-red-500/5 border border-red-500/20 text-red-400 text-xs font-medium hover:bg-red-500/10 transition-colors flex items-center gap-1.5"
                  >
                    <Trash2 className="h-3.5 w-3.5" /> Remove
                  </button>
                </div>
              </div>
            )}
          </div>
        </div>

        {/* ── Stripe bank account setup ─────────────────────────── */}
        <div className="glass rounded-xl p-5 space-y-4">
          <h2 className="text-sm font-semibold text-white flex items-center gap-2">
            <DollarSign className="h-4 w-4 text-green-400" />
            Stripe Payout Account Setup
          </h2>
          <p className="text-xs text-slate-400 leading-relaxed">
            Stripe collects compute credit payments from users and automatically
            pays out to your connected bank account every <strong className="text-white">2 business days</strong>.
            You must connect your bank account once in the Stripe Dashboard.
          </p>
          <ol className="space-y-2 text-xs text-slate-400 list-none">
            {[
              { n: "1", text: "Go to dashboard.stripe.com → Settings → Payouts" },
              { n: "2", text: "Click \"Add bank account\" — select Malaysia (MYR)" },
              { n: "3", text: "Enter your bank name, account number, and branch code (SWIFT/BIC)" },
              { n: "4", text: "Stripe verifies with a micro-deposit (1–2 business days)" },
              { n: "5", text: "Once verified, all user payments auto-payout to your account on a rolling T+2 schedule" },
            ].map((s) => (
              <li key={s.n} className="flex items-start gap-2.5">
                <span className="h-5 w-5 rounded-full bg-slate-800 border border-slate-700 text-slate-400 text-xs flex items-center justify-center flex-shrink-0 font-mono">{s.n}</span>
                <span className="pt-0.5">{s.text}</span>
              </li>
            ))}
          </ol>
          <div className="rounded-lg border border-green-400/20 bg-green-400/5 px-4 py-3 text-xs text-green-300">
            Malaysian banks supported: Maybank, CIMB, Public Bank, RHB, Hong Leong, AmBank, Alliance Bank.
            Stripe MY supports both MYR current and savings accounts.
          </div>
        </div>

        {/* ── CLI agent (hard hardware lock) ──────────────────────── */}
        <div className="glass rounded-xl p-5">
          <h2 className="text-sm font-semibold text-white mb-1 flex items-center gap-2">
            <ShieldCheck className="h-4 w-4 text-violet-400" />
            Hard Hardware Lock (CLI Agent)
          </h2>
          <p className="text-xs text-slate-500 mb-4">
            For providers, the CLI agent uses your Mac&apos;s <code className="font-mono text-slate-400">IOPlatformUUID</code> +
            serial number — a true hardware lock that even a full browser reset can&apos;t bypass.
          </p>
          <pre className="terminal text-xs leading-relaxed">{`# Creates an account bound to this Mac's serial number
nm account create --label "My Mac"

# Account ID is derived from:
#   BLAKE3(IOPlatformUUID || serial_number || ed25519_pubkey)
# Stored in macOS Keychain (Secure Enclave backed)

nm account status
# ✓ Account: a3f8b2c914d7...
# ✓ Device:  Apple M1 Max (32 GB)
# ✓ Hardware lock: active (serial: C02XY...)
# ✓ Identity: Ed25519 keypair (Keychain-backed)`}</pre>
        </div>

      </div>
    </div>
  );
}
