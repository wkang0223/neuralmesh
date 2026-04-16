import Stripe from "stripe";

let _stripe: Stripe | null = null;

// Lazy singleton — only instantiates at request time, not at build time.
export function getStripe(): Stripe {
  if (!_stripe) {
    if (!process.env.STRIPE_SECRET_KEY) {
      throw new Error("STRIPE_SECRET_KEY is not configured");
    }
    _stripe = new Stripe(process.env.STRIPE_SECRET_KEY, {
      apiVersion: "2026-03-25.dahlia",
    });
  }
  return _stripe;
}

// HC pricing constants
// 1 HC = RM 1.00 for Malaysian users (Stripe MY)
export const NMC_PRICE_MYR = 1.00;
export const MIN_DEPOSIT_NMC = 10;        // Minimum RM 10
export const ANNUAL_LIMIT_MYR_L1 = 5_000;   // BNM: RM 5,000/year for self-declared KYC
export const ANNUAL_LIMIT_MYR_L2 = 50_000;  // BNM: RM 50,000/year for verified KYC
