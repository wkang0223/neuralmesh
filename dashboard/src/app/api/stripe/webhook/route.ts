import { NextRequest, NextResponse } from "next/server";
import { getStripe } from "@/lib/stripe";
import Stripe from "stripe";

// Stripe sends this webhook after a successful payment.
// We credit the user's HC balance via the coordinator's internal ledger API.
// Stripe auto-pays out to your bank account separately (configured in Stripe Dashboard).
//
// To test locally: stripe listen --forward-to localhost:3000/api/stripe/webhook
export async function POST(req: NextRequest) {
  const body = await req.text();
  const sig  = req.headers.get("stripe-signature");

  if (!sig) {
    return NextResponse.json({ error: "Missing stripe-signature" }, { status: 400 });
  }

  let event: Stripe.Event;
  try {
    event = getStripe().webhooks.constructEvent(
      body,
      sig,
      process.env.STRIPE_WEBHOOK_SECRET!
    );
  } catch {
    return NextResponse.json({ error: "Invalid signature" }, { status: 400 });
  }

  if (event.type === "checkout.session.completed") {
    const session = event.data.object as Stripe.Checkout.Session;

    // Only process fully paid sessions (FPX can be async)
    if (session.payment_status !== "paid") return NextResponse.json({ received: true });

    const { account_id, amount_nmc, country } = session.metadata ?? {};
    if (!account_id || !amount_nmc) {
      console.error("Stripe webhook: missing metadata on session", session.id);
      return NextResponse.json({ error: "Missing metadata" }, { status: 400 });
    }

    const coordinatorUrl =
      process.env.COORDINATOR_URL ?? "http://localhost:8080";

    const creditRes = await fetch(
      `${coordinatorUrl}/api/v1/ledger/stripe-credit`,
      {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          // Internal secret so only this server can credit accounts
          "X-Internal-Secret": process.env.INTERNAL_API_SECRET ?? "",
        },
        body: JSON.stringify({
          account_id,
          amount_nmc: parseFloat(amount_nmc),
          stripe_session_id: session.id,
          stripe_payment_intent: session.payment_intent,
          // AMLA: record amount in MYR for annual limit tracking
          amount_myr: (session.amount_total ?? 0) / 100,
          country: country ?? "unknown",
          description: `Stripe deposit — session ${session.id}`,
        }),
      }
    );

    if (!creditRes.ok) {
      const text = await creditRes.text();
      console.error("Failed to credit HC after Stripe payment:", text);
      // Return 500 so Stripe retries the webhook
      return NextResponse.json({ error: "Credit failed" }, { status: 500 });
    }
  }

  // FPX payments can have a delay — handle async payment success
  if (event.type === "checkout.session.async_payment_succeeded") {
    const session = event.data.object as Stripe.Checkout.Session;
    const { account_id, amount_nmc } = session.metadata ?? {};
    if (account_id && amount_nmc) {
      const coordinatorUrl = process.env.COORDINATOR_URL ?? "http://localhost:8080";
      await fetch(`${coordinatorUrl}/api/v1/ledger/stripe-credit`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "X-Internal-Secret": process.env.INTERNAL_API_SECRET ?? "",
        },
        body: JSON.stringify({
          account_id,
          amount_nmc: parseFloat(amount_nmc),
          stripe_session_id: session.id,
          amount_myr: (session.amount_total ?? 0) / 100,
          country: session.metadata?.country ?? "unknown",
          description: `Stripe FPX deposit — session ${session.id}`,
        }),
      });
    }
  }

  return NextResponse.json({ received: true });
}
