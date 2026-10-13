import { AnchorProvider, Program, Wallet } from "@anchor-lang/core";
import { Connection, Keypair, PublicKey } from "@solana/web3.js";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const RPC_URL = process.env.RPC_URL ?? "https://api.devnet.solana.com";
export const DEPLOYMENT_FILE = resolve(ROOT, "deployments/devnet.json");

export const KEYS = {
  deployer: process.env.DEPLOYER_KEYPAIR ?? resolve(ROOT, ".keys/devnet-deployer.json"),
  keeper: resolve(ROOT, ".keys/devnet-keeper.json"),
  demoUser: resolve(ROOT, ".keys/devnet-demo-user.json"),
};

export function loadOrCreateKeypair(path: string): Keypair {
  if (existsSync(path)) {
    return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(readFileSync(path, "utf8"))));
  }
  const kp = Keypair.generate();
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, JSON.stringify(Array.from(kp.secretKey)), { mode: 0o600 });
  return kp;
}

export function program(name: "loyalty_buyback" | "mock_swap", signer: Keypair): Program {
  const idl = JSON.parse(readFileSync(resolve(ROOT, `target/idl/${name}.json`), "utf8"));
  const connection = new Connection(RPC_URL, "confirmed");
  const provider = new AnchorProvider(connection, new Wallet(signer), { commitment: "confirmed" });
  return new Program(idl, provider);
}

/** 32-byte id from a human-readable string (merchant ids, card tx ids). */
export function id32(value: string): number[] {
  return Array.from(createHash("sha256").update(value).digest());
}

export function pda(seeds: (Buffer | Uint8Array)[], programId: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(seeds, programId)[0];
}

export function explorer(sig: string): string {
  return `https://explorer.solana.com/tx/${sig}?cluster=devnet`;
}

export type Asset = { symbol: string; mint: string; decimals: number; priceUsd: number; tokenProgram: string };

export type Deployment = {
  rpc: string;
  loyaltyProgram: string;
  mockSwapProgram: string;
  admin: string;
  keeper: string;
  config: string;
  usdc: Asset;
  fallback: Asset;
  stocks: Asset[];
  poolAuthority: string;
  poolUsdc: string;
  merchants: { name: string; merchantId: number[]; address: string; vault: string; stock: string | null; cashbackBps: number }[];
};

export function readDeployment(): Deployment {
  if (!existsSync(DEPLOYMENT_FILE)) throw new Error("No deployments/devnet.json. Run `npm run deploy:devnet` first.");
  return JSON.parse(readFileSync(DEPLOYMENT_FILE, "utf8"));
}

export function writeDeployment(d: Deployment) {
  mkdirSync(dirname(DEPLOYMENT_FILE), { recursive: true });
  writeFileSync(DEPLOYMENT_FILE, JSON.stringify(d, null, 2) + "\n");
}
