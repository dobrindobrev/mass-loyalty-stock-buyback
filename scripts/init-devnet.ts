/**
 * Initializes a fresh devnet deployment for demos and testing:
 * test USDC, test stock tokens (Token-2022, like xStocks) and a test "SOL" token,
 * all minted by the mock swap's pool. Then config, demo merchants and their funding.
 * Writes every address to deployments/devnet.json.
 */
import { BN } from "@anchor-lang/core";
import {
  ASSOCIATED_TOKEN_PROGRAM_ID,
  TOKEN_2022_PROGRAM_ID,
  TOKEN_PROGRAM_ID,
  createMint,
  getAssociatedTokenAddressSync,
  getOrCreateAssociatedTokenAccount,
  mintTo,
} from "@solana/spl-token";
import { LAMPORTS_PER_SOL, PublicKey, SystemProgram, Transaction, sendAndConfirmTransaction } from "@solana/web3.js";
import { existsSync } from "node:fs";
import {
  Asset, DEPLOYMENT_FILE, Deployment, KEYS, RPC_URL, id32, loadOrCreateKeypair, pda, program, writeDeployment,
} from "./common.ts";

const USDC = 1_000_000;

const STOCKS = [
  { symbol: "tSBUX", priceUsd: 95 },
  { symbol: "tNKE", priceUsd: 75 },
  { symbol: "tAAPL", priceUsd: 230 },
];

// Demo merchants: three map to a stock, one has none (exercises the SOL fallback).
const MERCHANTS = [
  { name: "Corner Coffee", stock: "tSBUX", cashbackBps: 500 },
  { name: "Sneaker Store", stock: "tNKE", cashbackBps: 300 },
  { name: "Apple Store", stock: "tAAPL", cashbackBps: 200 },
  { name: "Local Bakery", stock: null, cashbackBps: 1_000 },
];
const MERCHANT_FUNDING = 1_000 * USDC;

async function main() {
  const admin = loadOrCreateKeypair(KEYS.deployer);
  const keeper = loadOrCreateKeypair(KEYS.keeper);
  const loyalty = program("loyalty_buyback", admin);
  const mockSwap = program("mock_swap", admin);
  const conn = loyalty.provider.connection;

  const config = pda([Buffer.from("config")], loyalty.programId);
  if (await conn.getAccountInfo(config)) {
    if (existsSync(DEPLOYMENT_FILE)) {
      console.log("Already initialized, see deployments/devnet.json");
      return;
    }
    throw new Error("Config exists on-chain but deployments/devnet.json is missing; redeploy with new program ids.");
  }

  console.log(`admin  ${admin.publicKey.toBase58()}`);
  console.log(`keeper ${keeper.publicKey.toBase58()}`);

  // The keeper pays rent for receipts and users' token accounts.
  if ((await conn.getBalance(keeper.publicKey)) < 0.1 * LAMPORTS_PER_SOL) {
    await sendAndConfirmTransaction(conn, new Transaction().add(
      SystemProgram.transfer({ fromPubkey: admin.publicKey, toPubkey: keeper.publicKey, lamports: 0.3 * LAMPORTS_PER_SOL }),
    ), [admin]);
    console.log("funded keeper with 0.3 SOL");
  }

  const poolAuthority = pda([Buffer.from("pool")], mockSwap.programId);

  const usdcMint = await createMint(conn, admin, admin.publicKey, null, 6, undefined, undefined, TOKEN_PROGRAM_ID);
  const fallbackMint = await createMint(conn, admin, poolAuthority, null, 9, undefined, undefined, TOKEN_PROGRAM_ID);
  const stocks: Asset[] = [];
  for (const s of STOCKS) {
    const mint = await createMint(conn, admin, poolAuthority, null, 8, undefined, undefined, TOKEN_2022_PROGRAM_ID);
    stocks.push({ symbol: s.symbol, mint: mint.toBase58(), decimals: 8, priceUsd: s.priceUsd, tokenProgram: TOKEN_2022_PROGRAM_ID.toBase58() });
    console.log(`${s.symbol.padEnd(6)} ${mint.toBase58()}`);
  }
  console.log(`tUSDC  ${usdcMint.toBase58()}`);
  console.log(`tSOL   ${fallbackMint.toBase58()}`);

  const poolUsdc = await getOrCreateAssociatedTokenAccount(conn, admin, usdcMint, poolAuthority, true);

  const initSig = await loyalty.methods
    .initializeConfig(keeper.publicKey, fallbackMint, mockSwap.programId)
    .accountsPartial({ admin: admin.publicKey, config, usdcMint, systemProgram: SystemProgram.programId })
    .rpc();
  console.log(`config ${config.toBase58()} (${initSig})`);

  // The deployer doubles as every demo merchant's authority and funds their vaults.
  const adminUsdc = await getOrCreateAssociatedTokenAccount(conn, admin, usdcMint, admin.publicKey);
  await mintTo(conn, admin, usdcMint, adminUsdc.address, admin, MERCHANTS.length * MERCHANT_FUNDING);

  const merchants: Deployment["merchants"] = [];
  for (const m of MERCHANTS) {
    const merchantId = id32(m.name);
    const merchant = pda([Buffer.from("merchant"), Buffer.from(merchantId)], loyalty.programId);
    const vault = getAssociatedTokenAddressSync(usdcMint, merchant, true, TOKEN_PROGRAM_ID);
    const stock = m.stock ? new PublicKey(stocks.find((s) => s.symbol === m.stock)!.mint) : null;

    await loyalty.methods
      .registerMerchant(merchantId, admin.publicKey, stock, m.cashbackBps, new BN(0))
      .accountsPartial({
        admin: admin.publicKey, config, merchant, usdcMint, vault,
        tokenProgram: TOKEN_PROGRAM_ID, associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID, systemProgram: SystemProgram.programId,
      })
      .rpc();
    await loyalty.methods
      .fundMerchant(new BN(MERCHANT_FUNDING))
      .accountsPartial({
        funder: admin.publicKey, config, merchant, usdcMint, funderTokenAccount: adminUsdc.address, vault, tokenProgram: TOKEN_PROGRAM_ID,
      })
      .rpc();
    merchants.push({ name: m.name, merchantId, address: merchant.toBase58(), vault: vault.toBase58(), stock: m.stock, cashbackBps: m.cashbackBps });
    console.log(`merchant "${m.name}" ${m.cashbackBps / 100}% -> ${m.stock ?? "SOL fallback"}, funded ${MERCHANT_FUNDING / USDC} USDC`);
  }

  writeDeployment({
    rpc: RPC_URL,
    loyaltyProgram: loyalty.programId.toBase58(),
    mockSwapProgram: mockSwap.programId.toBase58(),
    admin: admin.publicKey.toBase58(),
    keeper: keeper.publicKey.toBase58(),
    config: config.toBase58(),
    usdc: { symbol: "tUSDC", mint: usdcMint.toBase58(), decimals: 6, priceUsd: 1, tokenProgram: TOKEN_PROGRAM_ID.toBase58() },
    fallback: { symbol: "tSOL", mint: fallbackMint.toBase58(), decimals: 9, priceUsd: 150, tokenProgram: TOKEN_PROGRAM_ID.toBase58() },
    stocks,
    poolAuthority: poolAuthority.toBase58(),
    poolUsdc: poolUsdc.address.toBase58(),
    merchants,
  });
  console.log("\nWrote deployments/devnet.json");
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
