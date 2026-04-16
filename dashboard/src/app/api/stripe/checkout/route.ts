import { NextRequest, NextResponse } from "next/server";
import { getStripe, MIN_DEPOSIT_NMC } from "@/lib/stripe";

// POST /api/stripe/checkout
// Creates a Stripe Checkout session. Money goes to operator's Stripe account
// and auto-pays out to the connected bank account (T+2 business days).
// On success, Stripe fires the checkout.session.completed webhook which credits HC.
export async function POST(req: NextRequest) {
  try {
    const { account_id, amount_nmc, country } = await req.json();

    if (!account_id || typeof account_id !== "string") {
      return NextResponse.json({ error: "account_id required" }, { status: 400 });
    }
    if (!amount_nmc || amount_nmc < MIN_DEPOSIT_NMC) {
      return NextResponse.json(
        { error: `Minimum deposit is ${MIN_DEPOSIT_NMC} HC` },
        { status: 400 }
      );
    }

    // Amount in sen (MYR cents). 1 HC = RM 1.00 = 100 sen.
    const amount_sen = Math.round(amount_nmc * 100);

    // Payment methods: FPX is the primary Malaysian internet banking method.
    // Card is for international users. Both are Stripe-standard.
    const payment_method_types: string[] =
      country === "MY" ? ["fpx", "card"] : ["card"];

    const baseUrl = process.env.NEXTAUTH_URL ?? "http://localhost:3000";

    const session = await getStripe().checkout.sessions.create({
      payment_method_types: payment_method_types as never,
      line_items: [
        {
          price_data: {
            currency: "myr",
            product_data: {
              name: "Hatch Credits (HC)",
              description:
                `${amount_nmc} HC — non-transferable compute credit voucher. ` +
                "Redeemable exclusively for GPU compute time on Hatch. " +
                "Not a financial instrument or investment product.",
            },
            unit_amount: amount_sen,
          },
          quantity: 1,
        },
      ],
      mode: "payment",
      success_url: `${baseUrl}/wallet?deposit=success&session_id={CHECKOUT_SESSION_ID}`,
      cancel_url: `${baseUrl}/wallet?deposit=cancelled`,
      metadata: {
        account_id,
        amount_nmc: String(amount_nmc),
        // Carry country so webhook can log it for AMLA records
        country: country ?? "unknown",
      },
      // BNM AMLA: Stripe collects billing name/address at checkout
      billing_address_collection: "required",
      // Operator's bank receives payout via Stripe Dashboard → Settings → Payouts
    });

    return NextResponse.json({ url: session.url });
  } catch (err: unknown) {
    const message = err instanceof Error ? err.message : "Stripe error";
    return NextResponse.json({ error: message }, { status: 500 });
  }
}
