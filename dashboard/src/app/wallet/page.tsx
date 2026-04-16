"use client";

import { useEffect, useState } from "react";
import { useAccount, useReadContract } from "wagmi";
import { api, type Balance, type Transaction } from "@/lib/api-client";
import { cn } from "@/lib/utils";
import {
  DollarSign, TrendingUp, TrendingDown, Lock, Plus, ArrowUpRight,
  ShieldCheck, Cpu, Zap, Copy, CheckCheck, AlertCircle,
  ArrowDownToLine, Loader2,
} from "lucide-react";
import { toast } from "sonner";
import { ComplianceGate } from "@/components/ComplianceGate";
import { ConnectWallet } from "@/components/ConnectWallet";
import {
  CONTRACT_ADDRESSES, NMC_ABI, REGISTRY_ABI, PROVIDER_NFT_ABI,
  hasContracts, formatNmc, tierLabel, tierColor, shortAddr, activeChain,
} from "@/lib/web3";

// ── On-chain chain selector ───────────────────────────────────────────────────
type Chain = "solana" | "arbitrum";

// ── Quantum security badge ────────────────────────────────────────────────────
function QuantumBadge({ verified }: { verified: boolean }) {
  return (
    <div
      className={cn(
        "inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium border",
        verified
          ? "bg-violet-500/10 border-violet-500/30 text-violet-300"
          : "bg-slate-700/30 border-slate-600/30 text-slate-500"
      )}
    >
      <ShieldCheck className="h-3.5 w-3.5" />
      {verified ? "PQ-Secured (Dilithium3 + Ed25519)" : "Not PQ-verified"}
    </div>
  );
}

// ── Chain tab ─────────────────────────────────────────────────────────────────
function ChainTab({ chain, active, onClick }: { chain: Chain; active: boolean; onClick: () => void }) {
  const label = chain === "solana" ? "Solana" : "Arbitrum";
  const icon  = chain === "solana" ? "◎" : "⬡";
  return (
    <button
      onClick={onClick}
      className={cn(
        "flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors",
        active
          ? "bg-brand-400/15 border border-brand-400/30 text-brand-400"
          : "text-slate-400 hover:text-slate-300"
      )}
    >
      <span className="text-base leading-none">{icon}</span>
      {label}
    </button>
  );
}

// ── Provider NFT card ─────────────────────────────────────────────────────────
function ProviderNftCard({ chain }: { chain: Chain }) {
  return (
    <div className="glass rounded-xl p-5 glow-border">
      <div className="flex items-start justify-between mb-4">
        <div>
          <div className="text-xs text-slate-500 uppercase tracking-wider mb-1">Provider NFT</div>
          <div className="text-base font-semibold text-white">Hatch-GPU Soul-bound</div>
        </div>
        <div className="h-10 w-10 rounded-lg bg-gradient-to-br from-violet-500/30 to-brand-400/30 border border-violet-500/20 flex items-center justify-center">
          <Cpu className="h-5 w-5 text-violet-300" />
        </div>
      </div>
      <div className="text-sm text-slate-500 text-center py-4">
        No provider NFT on {chain === "solana" ? "Solana" : "Arbitrum"}.
        <br />
        <span className="text-xs">Run <code className="font-mono text-slate-400">nm provider register</code> to mint.</span>
      </div>
    </div>
  );
}

// ─── On-chain wallet section (wagmi-powered) ─────────────────────────────────

function OnChainSection({ chain }: { chain: Chain }) {
  const { address, isConnected } = useAccount();
  const isArbitrum = chain === "arbitrum";

  // Read on-chain HC balance + staking tier + provider NFT (only on Arbitrum)
  const nmcResult = useReadContract({
    address:  CONTRACT_ADDRESSES.nmc,
    abi:      NMC_ABI,
    functionName: "balanceOf",
    args:     address ? [address] : undefined,
    query:    { enabled: isConnected && isArbitrum && hasContracts && !!address },
  });

  const tierResult = useReadContract({
    address:  CONTRACT_ADDRESSES.registry,
    abi:      REGISTRY_ABI,
    functionName: "tierOf",
    args:     address ? [address] : undefined,
    query:    { enabled: isConnected && isArbitrum && hasContracts && !!address },
  });

  const isProviderResult = useReadContract({
    address:  CONTRACT_ADDRESSES.nft,
    abi:      PROVIDER_NFT_ABI,
    functionName: "isProvider",
    args:     address ? [address] : undefined,
    query:    { enabled: isConnected && isArbitrum && hasContracts && !!address },
  });

  const providerRecord = useReadContract({
    address:  CONTRACT_ADDRESSES.registry,
    abi:      REGISTRY_ABI,
    functionName: "providers",
    args:     address ? [address] : undefined,
    query:    { enabled: isConnected && isArbitrum && hasContracts && !!address },
  });

  const nmcBalance  = nmcResult.data as bigint | undefined;
  const tier        = tierResult.data as number | undefined;
  const isProvider  = isProviderResult.data as boolean | undefined;
  const stakeInfo   = providerRecord.data as readonly [bigint, bigint, bigint, number, boolean, `0x${string}`] | undefined;

  const explorerBase = activeChain.blockExplorers?.default.url ?? "https://sepolia.arbiscan.io";

  // ── Solana (off-chain bridge only) ──────────────────────────────────────────
  if (!isArbitrum) {
    return (
      <div className="glass rounded-xl p-5 glow-border">
        <div className="text-xs text-slate-500 uppercase tracking-wider mb-3">
          On-chain HC · Solana
        </div>
        <div className="flex items-center gap-3">
          <AlertCircle className="h-5 w-5 text-yellow-400 flex-shrink-0" />
          <div>
            <p className="text-sm text-slate-300">Solana bridge in development</p>
            <p className="text-xs text-slate-500 mt-0.5">
              Use the CLI: <code className="font-mono text-slate-400">nm wallet bridge-in &lt;amount&gt; --chain solana</code>
            </p>
          </div>
        </div>
      </div>
    );
  }

  // ── Arbitrum — disconnected ─────────────────────────────────────────────────
  if (!isConnected) {
    return (
      <div className="glass rounded-xl p-5 glow-border">
        <div className="flex items-start justify-between flex-wrap gap-4">
          <div>
            <div className="text-xs text-slate-500 uppercase tracking-wider mb-3">
              On-chain HC · {activeChain.name}
            </div>
            <p className="text-sm text-slate-300 mb-1">Connect a wallet to see your on-chain HC balance.</p>
            <p className="text-xs text-slate-500">
              Providers: staking tier, soul-bound NFT, and slashing history visible once connected.
            </p>
          </div>
          <QuantumBadge verified={false} />
        </div>
        <div className="mt-4">
          <ConnectWallet />
        </div>
      </div>
    );
  }

  // ── Arbitrum — connected ────────────────────────────────────────────────────
  const isLoading = nmcResult.isLoading || tierResult.isLoading;

  return (
    <div className="glass rounded-xl p-5 glow-border space-y-4">
      {/* Header */}
      <div className="flex items-start justify-between flex-wrap gap-4">
        <div>
          <div className="text-xs text-slate-500 uppercase tracking-wider mb-1">
            On-chain HC · {activeChain.name}
          </div>
          <ConnectWallet className="mt-2" />
        </div>
        <QuantumBadge verified={!!isProvider} />
      </div>

      {/* Balance + Tier row */}
      <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
        {/* HC balance */}
        <div className="bg-slate-900/50 rounded-lg p-3 col-span-2 sm:col-span-2">
          <div className="text-xs text-slate-500 mb-1">Wallet balance</div>
          {isLoading ? (
            <Loader2 className="h-5 w-5 text-slate-600 animate-spin" />
          ) : (
            <div className="text-2xl font-bold font-mono text-brand-400">
              {nmcBalance !== undefined ? formatNmc(nmcBalance) : "—"}
              <span className="text-sm font-normal text-slate-500 ml-1.5">HC</span>
            </div>
          )}
        </div>

        {/* Staking tier */}
        <div className="bg-slate-900/50 rounded-lg p-3">
          <div className="text-xs text-slate-500 mb-1">Stake tier</div>
          {isLoading ? (
            <Loader2 className="h-4 w-4 text-slate-600 animate-spin" />
          ) : (
            <div className={cn(
              "text-sm font-semibold border rounded-full px-2 py-0.5 inline-block",
              tierColor(tier ?? 0)
            )}>
              {tier !== undefined ? `T${tier} · ${tierLabel(tier)}` : "—"}
            </div>
          )}
        </div>

        {/* Staked amount */}
        <div className="bg-slate-900/50 rounded-lg p-3">
          <div className="text-xs text-slate-500 mb-1">Staked HC</div>
          {isLoading ? (
            <Loader2 className="h-4 w-4 text-slate-600 animate-spin" />
          ) : (
            <div className="text-sm font-mono text-slate-300">
              {stakeInfo ? formatNmc(stakeInfo[0]) : "—"}
            </div>
          )}
        </div>
      </div>

      {/* Unbonding notice */}
      {stakeInfo && stakeInfo[1] > 0n && (
        <div className="flex items-center gap-2 text-xs text-amber-400 bg-amber-500/10 rounded-lg px-3 py-2">
          <ArrowDownToLine className="h-3.5 w-3.5 flex-shrink-0" />
          <span>
            {formatNmc(stakeInfo[1])} HC unbonding — claimable{" "}
            {new Date(Number(stakeInfo[2]) * 1000).toLocaleDateString()}
          </span>
        </div>
      )}

      {/* Provider NFT status */}
      {isProvider !== undefined && (
        <div className={cn(
          "flex items-center gap-2 text-xs rounded-lg px-3 py-2",
          isProvider
            ? "bg-emerald-500/10 text-emerald-400"
            : "bg-slate-800/50 text-slate-500"
        )}>
          <Cpu className="h-3.5 w-3.5 flex-shrink-0" />
          {isProvider
            ? "Soul-bound ProviderNFT minted — active in job matching"
            : "No ProviderNFT. Run: nm provider register to mint one"}
        </div>
      )}

      {/* Contract links */}
      {hasContracts && (
        <div className="flex flex-wrap gap-3 text-xs text-slate-600 pt-1">
          {[
            { label: "HCToken",  addr: CONTRACT_ADDRESSES.nmc },
            { label: "Escrow",   addr: CONTRACT_ADDRESSES.escrow },
            { label: "Registry", addr: CONTRACT_ADDRESSES.registry },
          ].map(({ label, addr }) =>
            addr ? (
              <a
                key={label}
                href={`${explorerBase}/address/${addr}`}
                target="_blank"
                rel="noopener noreferrer"
                className="hover:text-slate-400 transition-colors underline underline-offset-2"
              >
                {label}↗
              </a>
            ) : null
          )}
        </div>
      )}
    </div>
  );
}

// ─── Main page ────────────────────────────────────────────────────────────────

export default function WalletPage() {
  const [accountId, setAccountId]       = useState<string | null>(null);
  const [balance, setBalance]           = useState<Balance | null>(null);
  const [txns, setTxns]                 = useState<Transaction[]>([]);
  const [loading, setLoading]           = useState(true);
  const [depositAmt, setDepositAmt]     = useState("50");
  const [withdrawAddr, setWithdrawAddr] = useState("");
  const [withdrawAmt, setWithdrawAmt]   = useState("10");
  const [chain, setChain]               = useState<Chain>("solana");
  const [copied, setCopied]             = useState(false);

  useEffect(() => {
    if (typeof window !== "undefined") {
      setAccountId(localStorage.getItem("nm_account_id"));
    }
  }, []);

  useEffect(() => {
    if (!accountId) { setLoading(false); return; }
    Promise.all([
      api.getBalance(accountId),
      api.listTransactions(accountId, 30),
    ])
      .then(([bal, { transactions }]) => {
        setBalance(bal);
        // Compute running balance_after for each transaction (newest first)
        // Start from current available + total_spent and walk backwards
        let running = (bal.available_nmc ?? 0) + (bal.escrowed_nmc ?? 0);
        const withBalance = transactions.map((tx) => {
          const result = { ...tx, balance_after: running };
          running -= tx.amount_nmc; // reverse out this tx
          return result;
        });
        setTxns(withBalance);
      })
      .catch(() => {})
      .finally(() => setLoading(false));
  }, [accountId]);

  async function handleDeposit() {
    const amt = parseFloat(depositAmt);
    if (isNaN(amt) || amt < 10) { toast.error("Minimum deposit is 10 HC (RM 10)"); return; }
    if (!accountId) { toast.error("Create an account first"); return; }
    try {
      // Detect country for FPX payment method
      const country = Intl.DateTimeFormat().resolvedOptions().timeZone.startsWith("Asia/Kuala")
        ? "MY" : "OTHER";

      const res = await fetch("/api/stripe/checkout", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ account_id: accountId, amount_nmc: amt, country }),
      });
      const data = await res.json();
      if (!res.ok) { toast.error(data.error ?? "Checkout failed"); return; }
      // Redirect to Stripe Checkout — payment goes to operator's Stripe → bank account
      window.location.href = data.url;
    } catch {
      toast.error("Deposit unavailable. Try: nm wallet deposit from CLI");
    }
  }

  async function handleWithdraw() {
    if (!accountId) { toast.error("Create an account first"); return; }
    const isEth = withdrawAddr.startsWith("0x") && withdrawAddr.length === 42;
    const isSol = withdrawAddr.length >= 32 && withdrawAddr.length <= 44 && !withdrawAddr.startsWith("0x");
    if (!isEth && !isSol) {
      toast.error("Enter a valid Ethereum (0x…) or Solana address");
      return;
    }
    const amt = parseFloat(withdrawAmt);
    if (isNaN(amt) || amt <= 0) { toast.error("Enter a valid amount"); return; }
    try {
      const data = await api.withdraw({
        account_id:          accountId,
        destination_address: withdrawAddr,
        amount_nmc:          amt,
        chain:               chain as "arbitrum" | "solana",
      });
      if (!data.ok) {
        toast.error(data.error ?? "Withdrawal failed");
        return;
      }
      toast.success(
        data.tx_id
          ? `Withdrawal submitted: ${data.tx_id.slice(0, 12)}…`
          : (data.message ?? "Withdrawal queued successfully")
      );
    } catch {
      toast.error("Withdrawal unavailable — use: nm wallet withdraw from CLI");
    }
  }

  function txColor(kind: string) {
    if (["deposit", "job_earning", "escrow_release"].includes(kind)) return "text-green-400";
    if (["withdrawal", "job_payment", "escrow_lock"].includes(kind)) return "text-red-400";
    return "text-slate-400";
  }

  function txSign(kind: string) {
    return ["deposit", "job_earning", "escrow_release"].includes(kind) ? "+" : "−";
  }

  return (
    <div className="min-h-screen pt-14">
      <div className="max-w-5xl mx-auto px-4 sm:px-6 py-8 space-y-8">

        {/* Header + chain selector */}
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
          <div>
            <h1 className="text-2xl font-bold text-white">HC Wallet</h1>
            <p className="text-sm text-slate-500 mt-1">
              Off-chain credits &amp; on-chain HC token · Quantum-resistant attestation
            </p>
          </div>
          <div className="flex items-center gap-3">
            <div className="flex gap-2">
              <ChainTab chain="solana"   active={chain === "solana"}   onClick={() => setChain("solana")} />
              <ChainTab chain="arbitrum" active={chain === "arbitrum"} onClick={() => setChain("arbitrum")} />
            </div>
            {chain === "arbitrum" && <ConnectWallet />}
          </div>
        </div>

        {/* No account prompt */}
        {!loading && !accountId && (
          <div className="glass rounded-xl p-6 flex items-center gap-4">
            <AlertCircle className="h-8 w-8 text-yellow-400 flex-shrink-0" />
            <div>
              <p className="text-white font-semibold">No account found</p>
              <p className="text-slate-400 text-sm mt-1">
                <a href="/account" className="text-brand-400 hover:text-brand-300">Create a device-linked account</a> to manage your HC balance.
              </p>
            </div>
          </div>
        )}

        {/* On-chain balance (wallet adapter placeholder) */}
        <OnChainSection chain={chain} />

        {/* Off-chain balance cards + Provider NFT */}
        <div className="grid grid-cols-1 sm:grid-cols-4 gap-4">
          <div className="glass rounded-xl p-5 sm:col-span-1">
            <DollarSign className="h-5 w-5 text-brand-400 mb-2" />
            <div className="text-3xl font-bold font-mono text-brand-400">
              {loading ? "—" : (balance?.available_nmc ?? 0).toFixed(4)}
            </div>
            <div className="text-xs text-slate-500 mt-1">Off-chain credits</div>
          </div>

          <div className="glass rounded-xl p-5 sm:col-span-1">
            <Lock className="h-5 w-5 text-yellow-400 mb-2" />
            <div className="text-2xl font-bold font-mono text-yellow-400">
              {loading ? "—" : (balance?.escrowed_nmc ?? 0).toFixed(4)}
            </div>
            <div className="text-xs text-slate-500 mt-1">In escrow</div>
          </div>

          <div className="glass rounded-xl p-5 sm:col-span-1">
            <TrendingUp className="h-5 w-5 text-green-400 mb-2" />
            <div className="text-2xl font-bold font-mono text-green-400">
              {loading ? "—" : (balance?.total_earned_nmc ?? 0).toFixed(4)}
            </div>
            <div className="text-xs text-slate-500 mt-1">Total earned</div>
          </div>

          <div className="sm:col-span-1">
            <ProviderNftCard chain={chain} />
          </div>
        </div>

        {/* Security info bar */}
        <div className="glass rounded-xl px-5 py-3.5 flex flex-wrap items-center gap-4 text-xs text-slate-400">
          <div className="flex items-center gap-1.5">
            <ShieldCheck className="h-4 w-4 text-violet-400" />
            <span>Hybrid Ed25519 + Dilithium3 (NIST FIPS 204) attestations</span>
          </div>
          <div className="flex items-center gap-1.5">
            <Zap className="h-4 w-4 text-yellow-400" />
            <span>ML-KEM-768 encrypted job channels</span>
          </div>
          <div className="flex items-center gap-1.5">
            <Cpu className="h-4 w-4 text-brand-400" />
            <span>BLAKE3 on-chain commitments · 2-of-3 oracle multisig</span>
          </div>
        </div>

        {/* Bridge actions */}
        <ComplianceGate accountId={accountId}>
        <div className="grid md:grid-cols-2 gap-6">

          {/* Deposit / Bridge-in */}
          <div className="glass rounded-xl p-5">
            <h2 className="text-sm font-semibold text-white mb-4 flex items-center gap-2">
              <Plus className="h-4 w-4 text-green-400" /> Add HC Credits
            </h2>
            <div className="space-y-3">
              <div>
                <label className="text-xs text-slate-500 block mb-1.5">Amount (HC)</label>
                <input
                  type="number" min="1" step="1"
                  value={depositAmt}
                  onChange={(e) => setDepositAmt(e.target.value)}
                  className="w-full px-3 py-2 rounded-lg bg-slate-900 border border-slate-700 text-sm text-slate-300 focus:outline-none focus:border-brand-400/50"
                />
              </div>
              <button
                onClick={handleDeposit}
                className="w-full py-2.5 rounded-lg bg-green-400/10 border border-green-400/20 text-green-400 text-sm font-medium hover:bg-green-400/20 transition-colors"
              >
                Pay via Stripe (FPX / Card) →
              </button>
              <p className="text-xs text-slate-600 text-center">
                1 HC = RM 1.00 · Stripe collects payment, funds deposited to operator bank account (T+2)
              </p>
              <div className="text-center text-xs text-slate-600 space-y-1">
                <p>Or bridge from {chain === "solana" ? "Solana" : "Arbitrum"}:</p>
                <p>
                  <code className="font-mono text-slate-500">
                    nm wallet bridge-in {depositAmt || "50"} --chain {chain}
                  </code>
                </p>
              </div>
            </div>
          </div>

          {/* Withdraw / Bridge-out */}
          <div className="glass rounded-xl p-5">
            <h2 className="text-sm font-semibold text-white mb-4 flex items-center gap-2">
              <ArrowUpRight className="h-4 w-4 text-yellow-400" />
              Withdraw to {chain === "solana" ? "Solana" : "Arbitrum"}
            </h2>
            <div className="space-y-3">
              <div>
                <label className="text-xs text-slate-500 block mb-1.5">
                  {chain === "solana" ? "Solana wallet address" : "Arbitrum address (0x…)"}
                </label>
                <input
                  type="text"
                  placeholder={chain === "solana" ? "7sXq…" : "0x…"}
                  value={withdrawAddr}
                  onChange={(e) => setWithdrawAddr(e.target.value)}
                  className="w-full px-3 py-2 rounded-lg bg-slate-900 border border-slate-700 text-sm text-slate-300 font-mono placeholder-slate-600 focus:outline-none focus:border-brand-400/50"
                />
              </div>
              <div>
                <label className="text-xs text-slate-500 block mb-1.5">Amount (HC)</label>
                <input
                  type="number" min="0.01" step="0.01"
                  value={withdrawAmt}
                  onChange={(e) => setWithdrawAmt(e.target.value)}
                  className="w-full px-3 py-2 rounded-lg bg-slate-900 border border-slate-700 text-sm text-slate-300 focus:outline-none focus:border-brand-400/50"
                />
              </div>
              <button
                onClick={handleWithdraw}
                className="w-full py-2.5 rounded-lg bg-yellow-400/10 border border-yellow-400/20 text-yellow-400 text-sm font-medium hover:bg-yellow-400/20 transition-colors"
              >
                Bridge out → {chain === "solana" ? "Solana HC" : "Arbitrum HC"}
              </button>
            </div>
          </div>
        </div>
        </ComplianceGate>

        {/* Transaction history */}
        <div className="glass rounded-xl overflow-hidden">
          <div className="px-5 py-4 border-b border-slate-800 flex items-center justify-between">
            <h2 className="text-sm font-semibold text-white">Transaction History</h2>
            <span className="text-xs text-slate-500">Off-chain ledger</span>
          </div>

          {loading ? (
            <div className="py-10 text-center text-slate-500 text-sm">Loading…</div>
          ) : txns.length === 0 ? (
            <div className="py-10 text-center text-slate-500 text-sm">No transactions yet.</div>
          ) : (
            <div className="divide-y divide-slate-800">
              {txns.map((tx) => (
                <div key={tx.id} className="px-5 py-3.5 flex items-center gap-4">
                  <div className={cn("flex-shrink-0", txColor(tx.kind))}>
                    {["deposit", "job_earning"].includes(tx.kind)
                      ? <TrendingUp className="h-4 w-4" />
                      : <TrendingDown className="h-4 w-4" />}
                  </div>
                  <div className="flex-1 min-w-0">
                    <div className="text-sm text-slate-300 truncate">{tx.description}</div>
                    <div className="text-xs text-slate-500 mt-0.5">
                      {new Date(tx.created_at).toLocaleString(undefined, {
                        month: "short", day: "numeric", hour: "2-digit", minute: "2-digit",
                      })}
                      {" · "}{tx.kind.replace(/_/g, " ")}
                    </div>
                  </div>
                  <div className="text-right">
                    <div className={cn("text-sm font-mono font-medium", txColor(tx.kind))}>
                      {txSign(tx.kind)}{Math.abs(tx.amount_nmc).toFixed(4)} HC
                    </div>
                    <div className="text-xs text-slate-500 mt-0.5">
                      bal: {tx.balance_after.toFixed(4)}
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

      </div>
    </div>
  );
}
