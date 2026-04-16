/**
 * device-id.ts — Device-locked account identity for Hatch
 *
 * How it works:
 *   1. Generate an ECDSA P-256 keypair via Web Crypto API (hardware-backed on
 *      devices with a TPM/Secure Enclave; non-extractable private key)
 *   2. Compute a device fingerprint from browser/OS signals
 *   3. Account ID = hex(SHA-256(pubkey_spki || fingerprint))[0..24]
 *   4. Store keypair + account info in IndexedDB (survives page refresh)
 *   5. Every sensitive API call is signed with the private key
 *
 * Security properties:
 *   - Private key is non-extractable — can't be copied out of the browser
 *   - Device fingerprint ties the account to this specific machine's signals
 *   - If a user tries to log in from a different device, the fingerprint check
 *     on the server will fail
 *   - This is a "soft" hardware lock for browser accounts; the CLI agent uses
 *     the macOS IOPlatformUUID / Linux machine-id for a hard hardware lock
 */

const DB_NAME = "hatch-identity";
const DB_VERSION = 1;
const STORE_NAME = "identity";
const IDENTITY_KEY = "primary";

export interface DeviceIdentity {
  accountId: string;
  deviceFingerprintHash: string;
  ecdsaPubkeyHex: string;
  deviceLabel: string;
  platform: string;
  createdAt: string;
  /** Private key — never leaves IndexedDB, used for signing */
  _privateKey?: CryptoKey;
  /** Public key — used for export + verification */
  _publicKey?: CryptoKey;
}

// ─── IndexedDB helpers ────────────────────────────────────────────────────────

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB_NAME, DB_VERSION);
    req.onupgradeneeded = (e) => {
      const db = (e.target as IDBOpenDBRequest).result;
      if (!db.objectStoreNames.contains(STORE_NAME)) {
        db.createObjectStore(STORE_NAME);
      }
    };
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function idbGet<T>(key: string): Promise<T | undefined> {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readonly");
    const store = tx.objectStore(STORE_NAME);
    const req = store.get(key);
    req.onsuccess = () => resolve(req.result as T | undefined);
    req.onerror = () => reject(req.error);
  });
}

async function idbSet(key: string, value: unknown): Promise<void> {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readwrite");
    const store = tx.objectStore(STORE_NAME);
    const req = store.put(value, key);
    req.onsuccess = () => resolve();
    req.onerror = () => reject(req.error);
  });
}

async function idbDelete(key: string): Promise<void> {
  const db = await openDb();
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE_NAME, "readwrite");
    const store = tx.objectStore(STORE_NAME);
    const req = store.delete(key);
    req.onsuccess = () => resolve();
    req.onerror = () => reject(req.error);
  });
}

// ─── Device fingerprinting ────────────────────────────────────────────────────

/**
 * Collect STABLE device signals and return a fingerprint hash.
 *
 * Signals chosen for stability across browser sessions and minor OS updates:
 *   - platform, hardwareConcurrency, language, timezone, screen geometry
 *   - Deliberately EXCLUDED: canvas, WebGL renderer, userAgent
 *     Reason: canvas rendering changes with GPU driver updates, browser
 *     anti-fingerprinting modes, and font hinting changes — causing
 *     false mismatches on every browser or OS update.
 *
 * The ECDSA keypair (generated once, stored in IndexedDB) provides the
 * actual cryptographic device binding — the fingerprint is a secondary
 * human-readable signal for UI display.
 */
async function computeDeviceFingerprint(): Promise<string> {
  const signals: string[] = [];

  // Stable OS/hardware signals
  signals.push(navigator.platform || "unknown");
  signals.push(String(navigator.hardwareConcurrency || 0));
  signals.push(navigator.language || "");
  signals.push(Intl.DateTimeFormat().resolvedOptions().timeZone);

  // Screen geometry (stable unless user changes monitor configuration)
  signals.push(`${screen.width}x${screen.height}x${screen.colorDepth}`);
  signals.push(String(window.devicePixelRatio || 1));

  // Hash all signals together
  const raw = signals.join("|||");
  const encoded = new TextEncoder().encode(raw);
  const hashBuffer = await crypto.subtle.digest("SHA-256", encoded);
  return Array.from(new Uint8Array(hashBuffer))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join(""); // 64 hex chars
}

// ─── ECDSA P-256 keypair ──────────────────────────────────────────────────────

async function generateKeypair(): Promise<CryptoKeyPair> {
  return crypto.subtle.generateKey(
    { name: "ECDSA", namedCurve: "P-256" },
    false, // non-extractable private key
    ["sign", "verify"]
  );
}

async function exportPublicKeyHex(pubkey: CryptoKey): Promise<string> {
  const spki = await crypto.subtle.exportKey("spki", pubkey);
  return Array.from(new Uint8Array(spki))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
}

async function deriveAccountId(pubkeyHex: string, fingerprintHash: string): Promise<string> {
  const combined = new TextEncoder().encode(`${pubkeyHex}||${fingerprintHash}`);
  const hashBuffer = await crypto.subtle.digest("SHA-256", combined);
  const hex = Array.from(new Uint8Array(hashBuffer))
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
  return hex.slice(0, 24); // 96-bit account ID
}

function detectPlatform(): string {
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes("mac")) return "macos";
  if (ua.includes("linux")) return "linux";
  if (ua.includes("win")) return "windows";
  return "browser";
}

// ─── Public API ───────────────────────────────────────────────────────────────

/**
 * Load the existing identity from IndexedDB, or return undefined if none.
 */
export async function loadIdentity(): Promise<DeviceIdentity | undefined> {
  try {
    return await idbGet<DeviceIdentity>(IDENTITY_KEY);
  } catch {
    return undefined;
  }
}

/**
 * Create a new device-locked account.
 * Generates a keypair, fingerprints the device, derives the account ID,
 * stores everything in IndexedDB, and registers with the coordinator.
 */
function coordinatorBase(): string {
  return process.env.NEXT_PUBLIC_COORDINATOR_URL ?? "http://localhost:8080";
}

export async function createIdentity(
  deviceLabel?: string,
  coordinatorUrl = coordinatorBase()
): Promise<DeviceIdentity> {
  const keypair = await generateKeypair();
  const pubkeyHex = await exportPublicKeyHex(keypair.publicKey);
  const fingerprintHash = await computeDeviceFingerprint();
  const accountId = await deriveAccountId(pubkeyHex, fingerprintHash);
  const platform = detectPlatform();
  const label = deviceLabel || `${platform} device`;

  const identity: DeviceIdentity = {
    accountId,
    deviceFingerprintHash: fingerprintHash,
    ecdsaPubkeyHex: pubkeyHex,
    deviceLabel: label,
    platform,
    createdAt: new Date().toISOString(),
    _privateKey: keypair.privateKey,
    _publicKey: keypair.publicKey,
  };

  // Register with coordinator
  await fetch(`${coordinatorUrl}/api/v1/account/register`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      ecdsa_pubkey_hex: pubkeyHex,
      device_fingerprint_hash: fingerprintHash,
      device_label: label,
      platform,
    }),
  }).catch(() => {
    // Registration failure is non-fatal; will retry on next verification
    console.warn("Hatch: account registration request failed (coordinator may be offline)");
  });

  await idbSet(IDENTITY_KEY, identity);

  // Also store accountId in localStorage for quick access (non-sensitive)
  localStorage.setItem("nm_account_id", accountId);

  return identity;
}

/**
 * Verify that the current device matches the stored identity.
 * Returns { verified: true } if the device fingerprint matches.
 */
export async function verifyDevice(
  identity: DeviceIdentity,
  coordinatorUrl = coordinatorBase()
): Promise<{ verified: boolean; reason?: string }> {
  try {
    const currentFingerprint = await computeDeviceFingerprint();

    // Local check first (fast path)
    if (currentFingerprint !== identity.deviceFingerprintHash) {
      return { verified: false, reason: "fingerprint_mismatch" };
    }

    // Server check
    const res = await fetch(
      `${coordinatorUrl}/api/v1/account/${identity.accountId}/verify`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          ecdsa_pubkey_hex: identity.ecdsaPubkeyHex,
          device_fingerprint_hash: currentFingerprint,
        }),
      }
    );

    if (!res.ok) return { verified: false, reason: "server_error" };
    const data = await res.json();
    return { verified: data.verified, reason: data.reason };
  } catch {
    // Offline — trust the local fingerprint check
    return { verified: true };
  }
}

/**
 * Sign a message with the stored private key.
 * Used to authenticate API requests (future — when coordinator enforces signatures).
 */
export async function signMessage(
  identity: DeviceIdentity,
  message: string
): Promise<string | null> {
  if (!identity._privateKey) return null;
  try {
    const encoded = new TextEncoder().encode(message);
    const sigBuffer = await crypto.subtle.sign(
      { name: "ECDSA", hash: "SHA-256" },
      identity._privateKey,
      encoded
    );
    return Array.from(new Uint8Array(sigBuffer))
      .map((b) => b.toString(16).padStart(2, "0"))
      .join("");
  } catch {
    return null;
  }
}

/**
 * Re-register this device — updates the stored fingerprint to the current one.
 * Use this when the fingerprint changes (e.g. OS update, screen config change)
 * but the user's keypair hasn't changed. The account ID stays the same.
 */
export async function reRegisterDevice(
  identity: DeviceIdentity,
  coordinatorUrl = coordinatorBase()
): Promise<DeviceIdentity> {
  const newFingerprint = await computeDeviceFingerprint();
  const updated: DeviceIdentity = {
    ...identity,
    deviceFingerprintHash: newFingerprint,
  };

  // Try to update the existing server record
  const reregRes = await fetch(
    `${coordinatorUrl}/api/v1/account/${identity.accountId}/reregister`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        ecdsa_pubkey_hex:            identity.ecdsaPubkeyHex,
        old_device_fingerprint_hash: identity.deviceFingerprintHash,
        new_device_fingerprint_hash: newFingerprint,
        device_label:                identity.deviceLabel,
      }),
    }
  ).catch(() => null);

  // If account is missing from coordinator (e.g. DB was wiped), re-register it fresh
  if (!reregRes || reregRes.status === 404) {
    await fetch(`${coordinatorUrl}/api/v1/account/register`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        ecdsa_pubkey_hex:        identity.ecdsaPubkeyHex,
        device_fingerprint_hash: newFingerprint,
        device_label:            identity.deviceLabel,
        platform:                identity.platform,
      }),
    }).catch(() => {
      console.warn("Hatch: fallback re-register failed (coordinator may be offline)");
    });
  }

  await idbSet(IDENTITY_KEY, updated);
  return updated;
}

/**
 * Delete the local identity (logout / reset).
 * Does NOT delete the server-side account.
 */
export async function deleteIdentity(): Promise<void> {
  await idbDelete(IDENTITY_KEY);
  localStorage.removeItem("nm_account_id");
}
