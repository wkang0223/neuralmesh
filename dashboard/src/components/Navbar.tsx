"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useState } from "react";
import { Cpu, Menu, X, Zap, ShieldCheck, ShieldAlert, UserCircle } from "lucide-react";
import { cn } from "@/lib/utils";
import { loadIdentity, verifyDevice, type DeviceIdentity } from "@/lib/device-id";

const NAV_LINKS = [
  { href: "/market",   label: "Browse GPUs" },
  { href: "/jobs",     label: "My Jobs" },
  { href: "/provider", label: "Provider" },
  { href: "/wallet",   label: "Wallet" },
  { href: "/account",  label: "Account" },
];

type DeviceStatus = "loading" | "none" | "verified" | "mismatch";

export default function Navbar() {
  const pathname = usePathname();
  const [open, setOpen] = useState(false);
  const [identity, setIdentity] = useState<DeviceIdentity | null>(null);
  const [deviceStatus, setDeviceStatus] = useState<DeviceStatus>("loading");

  useEffect(() => {
    loadIdentity().then(async (id) => {
      if (!id) { setDeviceStatus("none"); return; }
      setIdentity(id);
      const result = await verifyDevice(id);
      setDeviceStatus(result.verified ? "verified" : "mismatch");
    });
  }, []);

  return (
    <nav className="fixed top-0 inset-x-0 z-50 border-b border-slate-800/60 bg-surface-950/80 backdrop-blur-md">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
        <div className="flex items-center justify-between h-14">

          {/* Logo */}
          <Link href="/" className="flex items-center gap-2 group">
            <div className="relative">
              <Cpu className="h-6 w-6 text-brand-400 group-hover:text-brand-300 transition-colors" />
              <span className="absolute -top-0.5 -right-0.5 h-2 w-2 rounded-full bg-brand-400 animate-pulse" />
            </div>
            <span className="font-bold text-white tracking-tight">
              <span className="text-brand-400">Hatch</span>
            </span>
          </Link>

          {/* Desktop nav */}
          <div className="hidden md:flex items-center gap-1">
            {NAV_LINKS.map((link) => (
              <Link
                key={link.href}
                href={link.href}
                className={cn(
                  "px-3 py-1.5 rounded-md text-sm font-medium transition-colors",
                  pathname === link.href
                    ? "bg-brand-400/10 text-brand-300"
                    : "text-slate-400 hover:text-white hover:bg-slate-800"
                )}
              >
                {link.label}
              </Link>
            ))}
          </div>

          {/* Right side: device badge + CTA */}
          <div className="hidden md:flex items-center gap-3">

            {/* Device status pill — links to /account */}
            <Link href="/account">
              {deviceStatus === "verified" && identity && (
                <span className="flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs bg-green-500/10 border border-green-500/20 text-green-400 hover:bg-green-500/15 transition-colors cursor-pointer">
                  <ShieldCheck className="h-3 w-3" />
                  <span className="font-mono">{identity.accountId.slice(0, 8)}…</span>
                </span>
              )}
              {deviceStatus === "mismatch" && (
                <span className="flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs bg-red-500/10 border border-red-500/20 text-red-400 hover:bg-red-500/15 transition-colors cursor-pointer">
                  <ShieldAlert className="h-3 w-3" />
                  Device mismatch
                </span>
              )}
              {(deviceStatus === "none" || deviceStatus === "loading") && (
                <span className="flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs bg-slate-700/30 border border-slate-600/30 text-slate-500 hover:text-slate-400 hover:bg-slate-700/40 transition-colors cursor-pointer">
                  <UserCircle className="h-3 w-3" />
                  {deviceStatus === "loading" ? "…" : "Create account"}
                </span>
              )}
            </Link>

            <Link
              href="/market"
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-md text-sm font-medium bg-brand-400/10 text-brand-300 hover:bg-brand-400/20 border border-brand-400/20 transition-colors"
            >
              <Zap className="h-3.5 w-3.5" />
              Run a Job
            </Link>
          </div>

          {/* Mobile hamburger */}
          <button
            className="md:hidden p-2 rounded-md text-slate-400 hover:text-white"
            onClick={() => setOpen(!open)}
            aria-label="Toggle menu"
          >
            {open ? <X className="h-5 w-5" /> : <Menu className="h-5 w-5" />}
          </button>
        </div>
      </div>

      {/* Mobile menu */}
      {open && (
        <div className="md:hidden border-t border-slate-800 bg-surface-950/95 backdrop-blur-md">
          <div className="px-4 py-3 space-y-1">
            {NAV_LINKS.map((link) => (
              <Link
                key={link.href}
                href={link.href}
                onClick={() => setOpen(false)}
                className={cn(
                  "flex items-center justify-between px-3 py-2 rounded-md text-sm font-medium transition-colors",
                  pathname === link.href
                    ? "bg-brand-400/10 text-brand-300"
                    : "text-slate-400 hover:text-white hover:bg-slate-800"
                )}
              >
                {link.label}
                {link.href === "/account" && deviceStatus === "verified" && (
                  <ShieldCheck className="h-3.5 w-3.5 text-green-400" />
                )}
                {link.href === "/account" && deviceStatus === "mismatch" && (
                  <ShieldAlert className="h-3.5 w-3.5 text-red-400" />
                )}
              </Link>
            ))}
            <Link
              href="/market"
              onClick={() => setOpen(false)}
              className="block mt-2 px-3 py-2 rounded-md text-sm font-medium bg-brand-400 text-slate-950 hover:bg-brand-300 transition-colors text-center"
            >
              Run a Job
            </Link>
          </div>
        </div>
      )}
    </nav>
  );
}
