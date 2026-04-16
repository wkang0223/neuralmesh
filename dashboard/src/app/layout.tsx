import type { Metadata } from "next";
import "./globals.css";
import { Toaster } from "sonner";
import Navbar from "@/components/Navbar";
import { Web3Provider } from "@/components/Web3Provider";

export const metadata: Metadata = {
  title: "Hatch — Apple Silicon GPU Marketplace",
  description:
    "Lease idle Mac Mini and Mac Studio GPU compute. Run 70B LLMs on unified memory at a fraction of cloud costs.",
  keywords: ["apple silicon", "gpu marketplace", "MLX", "AI inference", "Mac Mini", "llm hosting"],
  openGraph: {
    title: "Hatch",
    description: "Apple Silicon GPU Marketplace",
    type: "website",
  },
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en" className="dark">
      <body className="min-h-screen bg-surface-950 text-slate-100 antialiased">
        <Web3Provider>
          <Navbar />
          <main>{children}</main>
        </Web3Provider>
        <Toaster
          position="bottom-right"
          theme="dark"
          toastOptions={{
            style: {
              background: "#0f172a",
              border: "1px solid #334155",
              color: "#e2e8f0",
            },
          }}
        />
      </body>
    </html>
  );
}
