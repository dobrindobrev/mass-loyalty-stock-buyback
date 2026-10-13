/**
 * Simulates what the backend does on a card transaction, against devnet:
 *   npm run simulate:devnet -- "Corner Coffee" 42.50 [userWallet]
 * Picks the merchant's stock if it has one, otherwise the SOL fallback, quotes it at a
 * fixed test price and calls process_cashback with the mock swap as the route.
 */
import { BN } from "@anchor-lang/core";
import { ASSOCIATED_TOKEN_PROGRAM_ID, getAccount, getAssociatedTokenAddressSync } from "@solana/spl-token";
import { PublicKey, SystemProgram } from "@solana/web3.js";
import { randomUUID } from "node:crypto";
import { KEYS, explorer, id32, loadOrCreateKeypair, pda, program, readDeployment } from "./common.ts";

const SLIPPAGE_BPS = 100;

async function main() {
  const [merchantName, amountArg, userArg] = process.argv.slice(2);
  const d = readDeployment();
  if (!merchantName || !amountArg) {
    console.log(`usage: npm run simulate:devnet -- "<merchant>" <amount USD> [user wallet]`);
    console.log(`merchants: ${d.merchants.map((m) => `"${m.name}"`).join(", ")}`);
    process.exit(1);
  }
  const m = d.merchants.find((x) => x.name.toLowerCase() === merchantName.toLowerCase());
  if (!m) throw new Error(`Unknown merchant "${merchantName}"`);

  const keeper = loadOrCreateKeypair(KEYS.keeper);
  const user = userArg ? new PublicKey(userArg) : loadOrCreateKeypair(KEYS.demoUser).publicKey;
  const loyalty = program("loyalty_buyback", keeper);
  const conn = loyalty.provider.connection;

  const usdc = d.usdc;
  const asset = (m.stock && d.stocks.find((s) => s.symbol === m.stock)) || d.fallback;
  const outputMint = new PublicKey(asset.mint);
  const outputProgram = new PublicKey(asset.tokenProgram);
  const usdcMint = new PublicKey(usdc.mint);
  const usdcProgram = new PublicKey(usdc.tokenProgram);

  // Same math as the program: purchase * bps / 10_000 (merchants here have no cap).
  const purchase = BigInt(Math.round(Number(amountArg) * 10 ** usdc.decimals));
  const cashback = (purchase * BigInt(m.cashbackBps)) / 10_000n;
  // Stand-in for a Jupiter quote: fixed test price.
  const quoted = BigInt(Math.floor((Number(cashback) / 10 ** usdc.decimals / asset.priceUsd) * 10 ** asset.decimals));
  const minOut = (quoted * BigInt(10_000 - SLIPPAGE_BPS)) / 10_000n;

  const merchant = new PublicKey(m.address);
  const vault = new PublicKey(m.vault);
  const userOutput = getAssociatedTokenAddressSync(outputMint, user, true, outputProgram);
  const cardTxId = id32(`card-tx-${randomUUID()}`);
  const receipt = pda([Buffer.from("receipt"), Buffer.from(cardTxId)], loyalty.programId);

  // Route accounts in mock_swap's `Swap` order; the program signs for the merchant PDA.
  const mockSwap = program("mock_swap", keeper);
  const swapData = mockSwap.coder.instruction.encode("swap", {
    amountIn: new BN(cashback.toString()),
    amountOut: new BN(quoted.toString()),
  });
  const route = [
    { pubkey: merchant, isSigner: false, isWritable: false },
    { pubkey: vault, isSigner: false, isWritable: true },
    { pubkey: usdcMint, isSigner: false, isWritable: false },
    { pubkey: new PublicKey(d.poolUsdc), isSigner: false, isWritable: true },
    { pubkey: outputMint, isSigner: false, isWritable: true },
    { pubkey: userOutput, isSigner: false, isWritable: true },
    { pubkey: new PublicKey(d.poolAuthority), isSigner: false, isWritable: false },
    { pubkey: usdcProgram, isSigner: false, isWritable: false },
    { pubkey: outputProgram, isSigner: false, isWritable: false },
  ];

  const before = await balance(conn, userOutput);
  console.log(`card payment  $${amountArg} at "${m.name}"`);
  console.log(`cashback      ${fmt(cashback, usdc.decimals)} USDC (${m.cashbackBps / 100}%) -> ${asset.symbol}${asset === d.fallback ? " (fallback)" : ""}`);

  const sig = await loyalty.methods
    .processCashback(cardTxId, new BN(purchase.toString()), new BN(minOut.toString()), swapData)
    .accountsPartial({
      keeper: keeper.publicKey,
      config: new PublicKey(d.config),
      merchant,
      usdcMint,
      vault,
      user,
      outputMint,
      userOutputAccount: userOutput,
      receipt,
      swapProgram: new PublicKey(d.mockSwapProgram),
      usdcTokenProgram: usdcProgram,
      outputTokenProgram: outputProgram,
      associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
      systemProgram: SystemProgram.programId,
    })
    .remainingAccounts(route)
    .rpc();

  const after = await balance(conn, userOutput);
  console.log(`bought        ${fmt(after - before, asset.decimals)} ${asset.symbol}`);
  console.log(`user ${user.toBase58()} now holds ${fmt(after, asset.decimals)} ${asset.symbol}`);
  console.log(`receipt       ${receipt.toBase58()}`);
  console.log(`tx            ${explorer(sig)}`);
}

async function balance(conn: import("@solana/web3.js").Connection, ata: PublicKey): Promise<bigint> {
  const info = await conn.getAccountInfo(ata);
  if (!info) return 0n;
  return (await getAccount(conn, ata, "confirmed", info.owner)).amount;
}

function fmt(v: bigint, decimals: number): string {
  return (Number(v) / 10 ** decimals).toLocaleString("en-US", { maximumFractionDigits: decimals });
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
