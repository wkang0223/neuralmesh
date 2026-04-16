// Hatch REST API client for the dashboard.
// All endpoints are served by the hatch coordinator on port 8080.
// (Phase 1: ledger/balance consolidated on coordinator; no separate ledger service.)

const COORDINATOR =
  process.env.NEXT_PUBLIC_COORDINATOR_URL ?? "http://localhost:8080";

// ── Types ─────────────────────────────────────────────────────────────────────

export interface Provider {
  id: string;
  chip_model: string;
  unified_memory_gb: number;
  gpu_cores: number;
  installed_runtimes: string[];
  floor_price_nmc_per_hour: number;
  trust_score: number;
  region?: string;
  bandwidth_mbps?: number;
  max_job_ram_gb?: number;
  state: "available" | "leased" | "offline";
  last_seen: string;
}

export interface Job {
  id: string;
  job_id: string;
  account_id: string;
  consumer_id: string;
  provider_id?: string;
  state:
    | "queued"
    | "matching"
    | "assigned"
    | "running"
    | "migrating"
    | "complete"
    | "failed"
    | "cancelled";
  runtime: string;
  min_ram_gb: number;
  max_price_per_hour: number;
  bundle_hash?: string;
  bundle_url?: string;
  output_hash?: string;
  actual_cost_nmc?: number;
  actual_runtime_s?: number;
  has_checkpoint?: boolean;
  checkpoint_iter?: number;
  failure_reason?: string;
  restore_attempts?: number;
  /** Exit code from the job process (0 = success, non-zero = error) */
  exit_code?: number;
  created_at: string;
  started_at?: string;
  completed_at?: string;
}

export interface NetworkStats {
  available_providers: number;
  active_providers: number;
  total_available_ram_gb: number;
  running_jobs: number;
  completed_jobs: number;
}

export interface Balance {
  account_id: string;
  available_nmc: number;
  escrowed_nmc: number;
  total_earned_nmc: number;
  total_spent_nmc: number;
}

export interface Transaction {
  id: string;
  kind: string;
  amount_nmc: number;
  /** Running balance after this transaction — computed client-side from the list */
  balance_after: number;
  description: string;
  reference?: string;
  created_at: string;
}

export interface JobSubmitRequest {
  account_id: string;
  runtime: string;
  min_ram_gb: number;
  max_duration_secs: number;
  max_price_per_hour: number;
  bundle_hash?: string;
  bundle_url?: string;
  preferred_region?: string;
  /** Optional script filename hint for dashboard-submitted jobs (ignored by coordinator) */
  script_name?: string;
}

export interface JobSubmitResponse {
  ok: boolean;
  job_id: string;
  state: string;
  estimated_wait_secs: number;
  locked_nmc: number;
  error?: string;
  message?: string;
}

export interface JobLogs {
  job_id: string;
  output: string;
  is_complete: boolean;
  offset: number;
}

export interface WithdrawRequest {
  account_id:          string;
  destination_address: string;
  amount_nmc:          number;
  chain:               "arbitrum" | "solana";
}

export interface WithdrawResponse {
  ok:     boolean;
  tx_id?: string;
  error?: string;
  message?: string;
}

export interface KycRecord {
  account_id: string;
  status: "not_submitted" | "pending" | "approved" | "rejected";
  compliance_level: number;
  full_name?: string;
  id_type?: string;
  country?: string;
  annual_limit_myr?: number;
  annual_deposited_myr?: number;
  submitted_at?: string;
  approved_at?: string;
  rejection_reason?: string;
}

// ── Fetch helpers ─────────────────────────────────────────────────────────────

async function get<T>(url: string): Promise<T> {
  const res = await fetch(url, { cache: "no-store" });
  if (!res.ok) throw new Error(`GET ${url} → ${res.status}`);
  return res.json();
}

async function post<T>(url: string, body: unknown): Promise<T> {
  const res = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
    cache: "no-store",
  });
  if (!res.ok) {
    const err = await res.text();
    throw new Error(err || `POST ${url} → ${res.status}`);
  }
  return res.json();
}

async function del(url: string): Promise<void> {
  const res = await fetch(url, { method: "DELETE", cache: "no-store" });
  if (!res.ok) throw new Error(`DELETE ${url} → ${res.status}`);
}

// ── API ───────────────────────────────────────────────────────────────────────

export interface ProviderListParams {
  min_ram_gb?: number;
  runtime?: string;
  max_price?: number;
  sort?: string;
  limit?: number;
  region?: string;
}

export const api = {
  // ── Providers ──────────────────────────────────────────────────────────────

  async listProviders(
    params: ProviderListParams = {}
  ): Promise<{ providers: Provider[]; total: number }> {
    const q = new URLSearchParams();
    if (params.min_ram_gb) q.set("min_ram", String(params.min_ram_gb));
    if (params.runtime)    q.set("runtime", params.runtime);
    if (params.region)     q.set("region",  params.region);
    q.set("limit", String(params.limit ?? 50));
    return get(`${COORDINATOR}/api/v1/providers?${q}`);
  },

  async getProvider(id: string): Promise<Provider> {
    return get(`${COORDINATOR}/api/v1/providers/${id}`);
  },

  // ── Network stats ──────────────────────────────────────────────────────────

  async getStats(): Promise<NetworkStats> {
    return get(`${COORDINATOR}/api/v1/stats`);
  },

  // ── Jobs ───────────────────────────────────────────────────────────────────

  async listJobs(
    accountId: string,
    limit = 20,
    state?: string
  ): Promise<{ jobs: Job[]; total: number }> {
    const q = new URLSearchParams({ account_id: accountId, limit: String(limit) });
    if (state) q.set("state", state);
    return get(`${COORDINATOR}/api/v1/jobs?${q}`);
  },

  async getJob(id: string): Promise<Job> {
    return get(`${COORDINATOR}/api/v1/jobs/${id}`);
  },

  async submitJob(
    req: JobSubmitRequest
  ): Promise<JobSubmitResponse> {
    return post(`${COORDINATOR}/api/v1/jobs`, req);
  },

  async cancelJob(id: string): Promise<void> {
    return del(`${COORDINATOR}/api/v1/jobs/${id}`);
  },

  async getJobLogs(id: string, offset = 0): Promise<JobLogs> {
    return get(`${COORDINATOR}/api/v1/jobs/${id}/logs?offset=${offset}`);
  },

  // ── Ledger (on coordinator, Phase 1) ──────────────────────────────────────

  async getBalance(accountId: string): Promise<Balance> {
    return get(`${COORDINATOR}/api/v1/balance/${accountId}`);
  },

  async listTransactions(
    accountId: string,
    limit = 20
  ): Promise<{ transactions: Transaction[]; total: number }> {
    return get(
      `${COORDINATOR}/api/v1/transactions?account_id=${accountId}&limit=${limit}`
    );
  },

  // ── KYC ────────────────────────────────────────────────────────────────────

  async getKyc(accountId: string): Promise<KycRecord> {
    return get(`${COORDINATOR}/api/v1/kyc/${accountId}`);
  },

  async submitKyc(data: {
    account_id: string;
    full_name: string;
    id_type: string;
    id_number: string;
    country: string;
  }): Promise<{ ok: boolean; status: string; message: string }> {
    return post(`${COORDINATOR}/api/v1/kyc/submit`, data);
  },

  // ── Account ────────────────────────────────────────────────────────────────

  async registerAccount(data: {
    ecdsa_pubkey_hex: string;
    device_fingerprint_hash: string;
    device_label?: string;
    platform?: string;
  }): Promise<{ ok: boolean; account_id: string; error?: string }> {
    return post(`${COORDINATOR}/api/v1/account/register`, data);
  },

  async getAccount(
    accountId: string
  ): Promise<{
    account_id: string;
    device_label?: string;
    platform?: string;
    role?: string;
    active?: boolean;
    balance?: Balance;
  }> {
    return get(`${COORDINATOR}/api/v1/account/${accountId}`);
  },

  async verifyDevice(
    accountId: string,
    data: { device_fingerprint_hash: string; ecdsa_pubkey_hex: string }
  ): Promise<{ ok: boolean; verified: boolean; reason?: string }> {
    return post(`${COORDINATOR}/api/v1/account/${accountId}/verify`, data);
  },

  // ── Withdraw ───────────────────────────────────────────────────────────────

  async withdraw(req: WithdrawRequest): Promise<WithdrawResponse> {
    return post(`${COORDINATOR}/api/v1/withdraw`, req);
  },
};
