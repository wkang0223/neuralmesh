"use client";

import { useState, useEffect } from "react";
import { ShieldCheck, AlertTriangle, CheckCircle, FileText, Lock } from "lucide-react";
import { toast } from "sonner";
import { cn } from "@/lib/utils";
import { api } from "@/lib/api-client";

// BNM-mandated terms the user must acknowledge
const TERMS = [
  "HC (Hatch Credit) is a non-transferable utility voucher redeemable exclusively for GPU compute time on the Hatch platform.",
  "HC is NOT a financial instrument, investment product, e-money, digital currency, or security as defined under Malaysian law.",
  "HC credits have no monetary value outside the Hatch platform and cannot be sold, traded, or exchanged between users.",
  "The value of GPU compute credits may change as pricing evolves. This is a pricing change, not an investment return.",
  "Hatch is not a bank, e-money issuer, or licensed financial institution. Compute credits are prepaid service vouchers.",
  "I understand the annual deposit limits applicable to my verification level and consent to AML transaction monitoring.",
  "I confirm that the identity information I provide is accurate and I consent to it being held for 7 years per AMLA 2001 s.22.",
];

const ID_TYPES = [
  { value: "mykad",    label: "MyKad (Malaysian IC)" },
  { value: "passport", label: "Passport" },
  { value: "nric",     label: "NRIC (non-Malaysian resident)" },
  { value: "other",    label: "Other government-issued ID" },
];

type Step = "country" | "identity" | "terms" | "done";

export default function CompliancePage() {
  const [accountId, setAccountId] = useState<string | null>(null);
  const [step, setStep]           = useState<Step>("country");
  const [submitting, setSubmitting] = useState(false);

  // Form state
  const [country, setCountry]     = useState("MY");
  const [fullName, setFullName]   = useState("");
  const [idType, setIdType]       = useState("mykad");
  const [idNumber, setIdNumber]   = useState("");
  const [checked, setChecked]     = useState<boolean[]>(TERMS.map(() => false));

  useEffect(() => {
    if (typeof window !== "undefined") {
      setAccountId(localStorage.getItem("nm_account_id"));
    }
  }, []);

  function allTermsAccepted() {
    return checked.every(Boolean);
  }

  async function handleSubmit() {
    if (!accountId) { toast.error("No account found. Create an account first."); return; }
    if (!fullName.trim()) { toast.error("Full name is required"); return; }
    if (!idNumber.trim()) { toast.error("ID number is required"); return; }
    if (!allTermsAccepted()) { toast.error("You must accept all terms"); return; }

    setSubmitting(true);
    try {
      // Hash the ID number client-side — raw number never leaves the browser
      const hashedId = await hashIdNumber(idNumber.trim(), accountId!);

      await api.submitKyc({
        account_id: accountId!,
        full_name:  fullName.trim(),
        id_type:    idType,
        id_number:  hashedId,    // coordinator hashes again server-side for double protection
        country:    country,
      });

      setStep("done");
      toast.success("Verification submitted successfully");
    } catch (err: unknown) {
      toast.error(err instanceof Error ? err.message : "Submission failed");
    } finally {
      setSubmitting(false);
    }
  }

  // SHA-256 hash the ID number client-side — raw PII never transmitted over the wire
  async function hashIdNumber(id: string, salt: string): Promise<string> {
    const data = new TextEncoder().encode(id + "|" + salt);
    const hash = await crypto.subtle.digest("SHA-256", data);
    return Array.from(new Uint8Array(hash)).map((b) => b.toString(16).padStart(2, "0")).join("");
  }

  if (!accountId) {
    return (
      <div className="min-h-screen pt-14 flex items-center justify-center">
        <div className="text-center space-y-3 max-w-sm px-4">
          <AlertTriangle className="h-10 w-10 text-yellow-400 mx-auto" />
          <p className="text-white font-semibold">No account found</p>
          <p className="text-slate-400 text-sm">
            <a href="/account" className="text-brand-400 hover:text-brand-300">Create a device-linked account</a> first.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen pt-14">
      <div className="max-w-xl mx-auto px-4 sm:px-6 py-10 space-y-8">

        {/* Header */}
        <div>
          <div className="flex items-center gap-2 mb-2">
            <ShieldCheck className="h-5 w-5 text-brand-400" />
            <h1 className="text-2xl font-bold text-white">Identity Verification</h1>
          </div>
          <p className="text-slate-400 text-sm">
            Required for financial transactions under Malaysian law (FSA 2013 · AMLA 2001).
          </p>
        </div>

        {/* Progress */}
        <div className="flex items-center gap-2">
          {(["country", "identity", "terms", "done"] as Step[]).map((s, i) => (
            <div key={s} className="flex items-center gap-2">
              <div className={cn(
                "h-7 w-7 rounded-full flex items-center justify-center text-xs font-bold border",
                step === s
                  ? "border-brand-400 bg-brand-400/10 text-brand-400"
                  : (["country","identity","terms","done"].indexOf(step) > i)
                    ? "border-green-400 bg-green-400/10 text-green-400"
                    : "border-slate-700 text-slate-500"
              )}>
                {["country","identity","terms","done"].indexOf(step) > i ? (
                  <CheckCircle className="h-4 w-4" />
                ) : (
                  i + 1
                )}
              </div>
              {i < 3 && <div className="flex-1 h-px bg-slate-800 w-8" />}
            </div>
          ))}
        </div>

        {/* Step 1: Country */}
        {step === "country" && (
          <div className="glass rounded-xl p-6 space-y-5">
            <h2 className="text-sm font-semibold text-white flex items-center gap-2">
              <FileText className="h-4 w-4 text-brand-400" />
              Where are you based?
            </h2>
            <div>
              <label className="text-xs text-slate-500 block mb-1.5">Country of residence</label>
              <select
                value={country}
                onChange={(e) => setCountry(e.target.value)}
                className="w-full px-3 py-2.5 rounded-lg bg-slate-900 border border-slate-700 text-sm text-slate-300 focus:outline-none focus:border-brand-400/50"
              >
                <option value="MY">Malaysia 🇲🇾</option>
                <option value="SG">Singapore 🇸🇬</option>
                <option value="US">United States 🇺🇸</option>
                <option value="GB">United Kingdom 🇬🇧</option>
                <option value="AU">Australia 🇦🇺</option>
                <option value="OTHER">Other country</option>
              </select>
            </div>

            {country === "MY" && (
              <div className="rounded-lg border border-yellow-400/20 bg-yellow-400/5 p-3 text-xs text-yellow-200/80 space-y-1">
                <p className="font-semibold text-yellow-300">Malaysian residents:</p>
                <p>• Annual deposit limit: RM 5,000 (Level 1) / RM 50,000 (Level 2 with verified documents)</p>
                <p>• MyKad or passport number required (hashed, never stored in plain text)</p>
                <p>• Transactions monitored per BNM AMLA guidelines</p>
              </div>
            )}

            <button
              onClick={() => setStep("identity")}
              className="w-full py-2.5 rounded-lg bg-brand-400 text-slate-950 font-semibold text-sm hover:bg-brand-300 transition-colors"
            >
              Continue →
            </button>
          </div>
        )}

        {/* Step 2: Identity */}
        {step === "identity" && (
          <div className="glass rounded-xl p-6 space-y-4">
            <h2 className="text-sm font-semibold text-white flex items-center gap-2">
              <Lock className="h-4 w-4 text-brand-400" />
              Identity details
            </h2>

            <div>
              <label className="text-xs text-slate-500 block mb-1.5">
                Full legal name (as on your ID)
              </label>
              <input
                type="text"
                value={fullName}
                onChange={(e) => setFullName(e.target.value)}
                placeholder="e.g. Ahmad bin Abdullah"
                className="w-full px-3 py-2.5 rounded-lg bg-slate-900 border border-slate-700 text-sm text-slate-300 placeholder-slate-600 focus:outline-none focus:border-brand-400/50"
              />
            </div>

            <div>
              <label className="text-xs text-slate-500 block mb-1.5">ID type</label>
              <select
                value={idType}
                onChange={(e) => setIdType(e.target.value)}
                className="w-full px-3 py-2.5 rounded-lg bg-slate-900 border border-slate-700 text-sm text-slate-300 focus:outline-none focus:border-brand-400/50"
              >
                {ID_TYPES.map((t) => (
                  <option key={t.value} value={t.value}>{t.label}</option>
                ))}
              </select>
            </div>

            <div>
              <label className="text-xs text-slate-500 block mb-1.5">
                {idType === "mykad" ? "MyKad number (e.g. 900101-14-5678)" : "ID / Passport number"}
              </label>
              <input
                type="text"
                value={idNumber}
                onChange={(e) => setIdNumber(e.target.value)}
                placeholder={idType === "mykad" ? "YYMMDD-SS-NNNN" : ""}
                className="w-full px-3 py-2.5 rounded-lg bg-slate-900 border border-slate-700 text-sm text-slate-300 font-mono placeholder-slate-600 focus:outline-none focus:border-brand-400/50"
              />
              <p className="text-xs text-slate-600 mt-1">
                Your ID number is hashed (SHA-256) in your browser before being sent. It is never stored in plain text.
              </p>
            </div>

            <div className="flex gap-3">
              <button
                onClick={() => setStep("country")}
                className="flex-1 py-2.5 rounded-lg border border-slate-700 text-slate-400 text-sm hover:text-white hover:border-slate-600 transition-colors"
              >
                ← Back
              </button>
              <button
                onClick={() => {
                  if (!fullName.trim() || !idNumber.trim()) {
                    toast.error("Please fill in all fields");
                    return;
                  }
                  setStep("terms");
                }}
                className="flex-1 py-2.5 rounded-lg bg-brand-400 text-slate-950 font-semibold text-sm hover:bg-brand-300 transition-colors"
              >
                Continue →
              </button>
            </div>
          </div>
        )}

        {/* Step 3: Terms acknowledgment */}
        {step === "terms" && (
          <div className="glass rounded-xl p-6 space-y-4">
            <h2 className="text-sm font-semibold text-white flex items-center gap-2">
              <FileText className="h-4 w-4 text-brand-400" />
              Acknowledgments
            </h2>
            <p className="text-xs text-slate-500">
              You must understand and accept each statement individually.
            </p>

            <div className="space-y-3 max-h-96 overflow-y-auto pr-1">
              {TERMS.map((term, i) => (
                <label
                  key={i}
                  className={cn(
                    "flex items-start gap-3 p-3 rounded-lg border cursor-pointer transition-colors",
                    checked[i]
                      ? "border-green-400/30 bg-green-400/5"
                      : "border-slate-700 hover:border-slate-600"
                  )}
                >
                  <input
                    type="checkbox"
                    checked={checked[i]}
                    onChange={(e) => {
                      const next = [...checked];
                      next[i] = e.target.checked;
                      setChecked(next);
                    }}
                    className="mt-0.5 accent-green-400 flex-shrink-0"
                  />
                  <span className="text-xs text-slate-300 leading-relaxed">{term}</span>
                </label>
              ))}
            </div>

            <div className="flex gap-3">
              <button
                onClick={() => setStep("identity")}
                className="flex-1 py-2.5 rounded-lg border border-slate-700 text-slate-400 text-sm hover:text-white hover:border-slate-600 transition-colors"
              >
                ← Back
              </button>
              <button
                onClick={handleSubmit}
                disabled={!allTermsAccepted() || submitting}
                className="flex-1 py-2.5 rounded-lg bg-brand-400 text-slate-950 font-semibold text-sm hover:bg-brand-300 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
              >
                {submitting ? "Submitting…" : "Submit verification"}
              </button>
            </div>
          </div>
        )}

        {/* Done */}
        {step === "done" && (
          <div className="glass rounded-xl p-8 text-center space-y-4">
            <CheckCircle className="h-12 w-12 text-green-400 mx-auto" />
            <h2 className="text-xl font-bold text-white">Verification complete</h2>
            <div className="text-sm text-slate-400 space-y-1">
              <p>KYC Level 1 approved. Annual deposit limit: <span className="text-white font-semibold">RM 5,000</span></p>
              <p className="text-xs text-slate-500 mt-2">
                To increase your limit to RM 50,000, email{" "}
                <a href="mailto:compliance@hatch.dev" className="text-brand-400">
                  compliance@hatch.dev
                </a>{" "}
                with a copy of your {idType === "mykad" ? "MyKad" : "passport"}.
              </p>
            </div>
            <a
              href="/wallet"
              className="inline-block px-6 py-2.5 rounded-lg bg-brand-400 text-slate-950 font-semibold text-sm hover:bg-brand-300 transition-colors"
            >
              Go to Wallet →
            </a>
          </div>
        )}

        {/* Legal footer */}
        <p className="text-xs text-slate-600 text-center leading-relaxed">
          Hatch collects identity information to comply with Malaysia's{" "}
          <strong className="text-slate-500">Financial Services Act 2013</strong>,{" "}
          <strong className="text-slate-500">Anti-Money Laundering Act 2001</strong>, and{" "}
          <strong className="text-slate-500">Bank Negara Malaysia guidelines</strong>.{" "}
          Records are retained for 7 years. Your data is encrypted at rest and never sold.
        </p>
      </div>
    </div>
  );
}
