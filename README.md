# NeuralMesh

**Decentralised compute marketplace for Apple Silicon.**  
Idle Mac Minis and Mac Studios lease their unified GPU memory to ML workloads worldwide — compensated in NMC credits.

> Phase 1 focus: Apple M-series (M1 → M4 Ultra). The only platform that treats unified memory as a first-class GPU resource.

---

## Why Apple Silicon?

| Mac | Unified Memory | GPU Cores | Best For |
|---|---|---|---|
| Mac Mini M4 | 16–24 GB | 10 | Small model inference |
| Mac Mini M4 Pro | 24–64 GB | 20 | 30B LLM inference |
| Mac Studio M4 Max | 64–128 GB | 40 | 70B LLM inference |
| Mac Studio M4 Ultra | 128–192 GB | 80 | Full 405B inference shards |

A single Mac Mini M4 Pro (48 GB) runs Llama 3 70B fully in Metal/MPS — equivalent to two A100 80 GB cards at a fraction of the cost.

No existing platform (io.net, Vast.ai, Salad, Akash) supports Apple Silicon. NeuralMesh fills that gap.

---

## Architecture

```
Provider (idle Mac)          Coordinator (Rust)          Consumer (any machine)
┌──────────────────┐        ┌──────────────────┐        ┌──────────────────┐
│ neuralmesh-agent │──gRPC──│  Job matching    │──gRPC──│ neuralmesh-cli   │
│ • IOKit idle det.│        │  Kademlia DHT    │        │ nm job submit    │
│ • MLX / PyTorch  │        │  NATS JetStream  │        │ nm gpu list      │
│ • sandbox-exec   │        │  PostgreSQL       │        │ nm wallet        │
│ • WireGuard      │        │  REST + gRPC      │        └──────────────────┘
└──────────────────┘        └──────────────────┘
                                    │
                            ┌──────────────────┐
                            │  Dashboard       │
                            │  Next.js 15      │
                            │  Market, Jobs,   │
                            │  Wallet, KYC     │
                            └──────────────────┘
```

---

## Monorepo Structure

```
neuralmesh/
├── agent/              # macOS provider daemon (launchd)
├── coordinator/        # Job matching, REST + gRPC API
├── ledger/             # Off-chain credit accounting
├── cli/                # nm CLI for providers and consumers
├── dashboard/          # Next.js 15 web dashboard
├── crates/
│   ├── nm-common/      # Shared types
│   ├── nm-crypto/      # Ed25519, attestation
│   ├── nm-macos/       # IOKit, CGSession, Metal, sandbox-exec
│   ├── nm-proto/       # Generated gRPC stubs
│   └── nm-wireguard/   # boringtun WireGuard wrapper
├── proto/              # agent.proto, job.proto, ledger.proto
├── infrastructure/     # Dockerfiles, docker-compose, k8s
└── scripts/            # Install scripts, cross-compile helpers
```

---

## Quick Start

### Run locally (development)

**Prerequisites:** Rust 1.78+, PostgreSQL, Node 20+

```bash
# 1. Start infrastructure
docker compose -f infrastructure/docker-compose.yml up -d

# 2. Run coordinator
DATABASE_URL=postgresql://neuralmesh:neuralmesh@localhost/neuralmesh \
  cargo run -p neuralmesh-coordinator

# 3. Run dashboard
cd dashboard && npm install && npm run dev
# → http://localhost:3000
```

### Register your Mac as a provider

```bash
# Install ML runtimes (one-time)
python3.12 -m pip install mlx mlx-lm

# Start the agent
cargo run -p neuralmesh-agent -- --foreground
```

The agent auto-detects your chip, registers with the coordinator, and offers your GPU when the screen is locked and GPU utilisation is below 5%.

---

## Technology Stack

| Layer | Technology |
|---|---|
| Agent | Rust, IOKit FFI, CGSession, Metal, sandbox-exec |
| Coordinator | Rust, Axum (REST), Tonic (gRPC), libp2p, SQLx |
| Job queue | NATS JetStream (optional, falls back to DB polling) |
| Database | PostgreSQL (SQLx migrations) |
| Dashboard | Next.js 15, Tailwind CSS, Recharts |
| Payments | Stripe Checkout + FPX (Malaysian online banking) |
| Compliance | BNM KYC tiers, AMLA 2001 STR obligations |
| Networking | WireGuard (boringtun), libp2p Kademlia |

---

## Deployment (Free Tier)

| Service | Platform | Plan |
|---|---|---|
| Coordinator | [Render](https://render.com) | Free (Docker) |
| PostgreSQL | Render managed DB | Free |
| Dashboard | [Vercel](https://vercel.com) | Free |
| Redis (optional) | [Upstash](https://upstash.com) | Free |

See `render.yaml` and `dashboard/vercel.json` for configuration.

---

## Roadmap

| Phase | Scope | Status |
|---|---|---|
| 1 | Apple Silicon MVP — agent, coordinator, dashboard, off-chain credits | **In progress** |
| 2 | DMTCP job migration, macOS Virtualization.framework, reputation system | Planned |
| 3 | Arbitrum L2 token (NMC ERC-20), ZK Groth16 compute proofs, on-chain escrow | Planned |
| 4 | NVIDIA / AMD / Intel Arc (Linux), Docker + Firecracker isolation | Planned |
| 5 | Python SDK, HuggingFace integration, DAO governance | Planned |

---

## Security

- Jobs run as a dedicated `neuralmesh_worker` OS user under `sandbox-exec`
- File access restricted to `/tmp/neuralmesh/job-{id}/` only
- Network access: localhost only (no internet from inside job)
- Per-job ephemeral WireGuard tunnels (ChaCha20-Poly1305)
- Provider identity: Ed25519 keypair stored in macOS Keychain (Secure Enclave)
- Sybil resistance: IOPlatformSerialNumber + IOPlatformUUID hardware attestation
- Malaysian compliance: BNM KYC tiers, AMLA STR auto-flagging at RM 3,000/day

---

## Contributing

Pull requests welcome. Please open an issue first for significant changes.

## License

MIT — see [LICENSE](LICENSE)
