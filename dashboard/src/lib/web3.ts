/**
 * Hatch — wagmi 2 + viem configuration.
 *
 * Supports:
 *   • Arbitrum Sepolia (testnet, chain id 421614) — default for dev
 *   • Arbitrum One    (mainnet, chain id 42161)  — prod
 *
 * Connectors:
 *   • injected() — MetaMask, Rabby, Frame, etc.
 *
 * ABI fragments for HCToken (ERC-20) and Registry (staking).
 */

import { createConfig, http }    from "wagmi";
import { arbitrum, arbitrumSepolia } from "wagmi/chains";
import { injected }              from "wagmi/connectors";
import { createPublicClient }    from "viem";

// ── Chain selection ───────────────────────────────────────────────────────────

export const TESTNET_CHAIN_ID = 421614; // Arbitrum Sepolia
export const MAINNET_CHAIN_ID = 42161;  // Arbitrum One

/** True when the dashboard is configured for Arbitrum mainnet. */
export const isMainnet =
  process.env.NEXT_PUBLIC_CHAIN_ID === String(MAINNET_CHAIN_ID);

export const activeChain = isMainnet ? arbitrum : arbitrumSepolia;

// ── wagmi config ──────────────────────────────────────────────────────────────

export const wagmiConfig = createConfig({
  chains: [arbitrumSepolia, arbitrum],
  connectors: [
    injected(), // MetaMask, Rabby, Frame, etc.
  ],
  transports: {
    [arbitrumSepolia.id]: http(
      process.env.NEXT_PUBLIC_ARB_SEPOLIA_RPC ??
        "https://sepolia-rollup.arbitrum.io/rpc"
    ),
    [arbitrum.id]: http(
      process.env.NEXT_PUBLIC_ARB_RPC ?? "https://arb1.arbitrum.io/rpc"
    ),
  },
  ssr: true, // Next.js server-side rendering compatibility
});

// ── Public client (read-only) ─────────────────────────────────────────────────

export const publicClient = createPublicClient({
  chain: activeChain,
  transport: http(
    isMainnet
      ? (process.env.NEXT_PUBLIC_ARB_RPC ?? "https://arb1.arbitrum.io/rpc")
      : (process.env.NEXT_PUBLIC_ARB_SEPOLIA_RPC ??
          "https://sepolia-rollup.arbitrum.io/rpc")
  ),
});

// ── Contract addresses ────────────────────────────────────────────────────────

export const CONTRACT_ADDRESSES = {
  nmc:      (process.env.NEXT_PUBLIC_NMC_ADDRESS      ?? "") as `0x${string}`,
  escrow:   (process.env.NEXT_PUBLIC_ESCROW_ADDRESS   ?? "") as `0x${string}`,
  registry: (process.env.NEXT_PUBLIC_REGISTRY_ADDRESS ?? "") as `0x${string}`,
  nft:      (process.env.NEXT_PUBLIC_PROVIDER_NFT_ADDRESS ?? "") as `0x${string}`,
  qv:       (process.env.NEXT_PUBLIC_QV_ADDRESS       ?? "") as `0x${string}`,
} as const;

export const hasContracts = CONTRACT_ADDRESSES.nmc.startsWith("0x");

// ── ABI fragments ─────────────────────────────────────────────────────────────

/** HCToken — ERC-20 balance + allowance reads */
export const NMC_ABI = [
  {
    name:    "balanceOf",
    type:    "function",
    stateMutability: "view",
    inputs:  [{ name: "account", type: "address" }],
    outputs: [{ name: "",        type: "uint256" }],
  },
  {
    name:    "decimals",
    type:    "function",
    stateMutability: "view",
    inputs:  [],
    outputs: [{ name: "", type: "uint8" }],
  },
  {
    name:    "approve",
    type:    "function",
    stateMutability: "nonpayable",
    inputs:  [
      { name: "spender", type: "address" },
      { name: "amount",  type: "uint256" },
    ],
    outputs: [{ name: "", type: "bool" }],
  },
] as const;

/** Registry — staking tier reads */
export const REGISTRY_ABI = [
  {
    name:    "tierOf",
    type:    "function",
    stateMutability: "view",
    inputs:  [{ name: "provider", type: "address" }],
    outputs: [{ name: "",         type: "uint8"   }],
  },
  {
    name:    "isActive",
    type:    "function",
    stateMutability: "view",
    inputs:  [{ name: "provider", type: "address" }],
    outputs: [{ name: "",         type: "bool"    }],
  },
  {
    name:    "providers",
    type:    "function",
    stateMutability: "view",
    inputs:  [{ name: "", type: "address" }],
    outputs: [
      { name: "stakedNmc",       type: "uint256" },
      { name: "unbondingNmc",    type: "uint256" },
      { name: "unbondingEndsAt", type: "uint64"  },
      { name: "slashCount",      type: "uint32"  },
      { name: "active",          type: "bool"    },
      { name: "providerIdPrefix",type: "bytes20"  },
    ],
  },
  {
    name:    "registerAndStake",
    type:    "function",
    stateMutability: "nonpayable",
    inputs:  [
      { name: "amount",           type: "uint256" },
      { name: "providerIdPrefix", type: "bytes20"  },
    ],
    outputs: [],
  },
  {
    name:    "queueUnstake",
    type:    "function",
    stateMutability: "nonpayable",
    inputs:  [{ name: "amount", type: "uint256" }],
    outputs: [],
  },
  {
    name:    "claimUnstake",
    type:    "function",
    stateMutability: "nonpayable",
    inputs:  [],
    outputs: [],
  },
] as const;

/** ProviderNFT — soul-bound identity */
export const PROVIDER_NFT_ABI = [
  {
    name:    "isProvider",
    type:    "function",
    stateMutability: "view",
    inputs:  [{ name: "addr", type: "address" }],
    outputs: [{ name: "",     type: "bool"    }],
  },
] as const;

// ── Formatting helpers ────────────────────────────────────────────────────────

/** Convert HC wei (18 decimals) to a human-readable float. */
export function formatNmc(wei: bigint): string {
  const divisor = 10n ** 18n;
  const whole   = wei / divisor;
  const frac    = wei % divisor;
  const fracStr = frac.toString().padStart(18, "0").slice(0, 4);
  return `${whole.toLocaleString()}.${fracStr}`;
}

/** Tier label from on-chain uint8. */
export function tierLabel(tier: number): string {
  return ["—", "Standard", "Verified", "Elite"][tier] ?? "Unknown";
}

/** Tier colour class for badges. */
export function tierColor(tier: number): string {
  return [
    "text-slate-500 border-slate-600",
    "text-emerald-400 border-emerald-500/40",
    "text-brand-400 border-brand-400/40",
    "text-amber-400 border-amber-500/40",
  ][tier] ?? "text-slate-500";
}

/** Shorten 0x address to 0x1234…5678 */
export function shortAddr(addr: string): string {
  if (!addr || addr.length < 10) return addr;
  return `${addr.slice(0, 6)}…${addr.slice(-4)}`;
}
