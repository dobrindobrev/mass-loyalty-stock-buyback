# Mass Loyalty: Stock Buyback

Card cashback that turns into ownership. Every card payment triggers merchant-funded USDC cashback. That cashback is automatically used to buy the tokenized stock of the brand the user just paid, falling back to SOL, and it builds up in the user's wallet.

```
card payment ──► backend (keeper) ──► process_cashback ──► swap USDC → stock token ──► user's wallet
                                           │                     └─ or → SOL if no stock / no route
                                           └─ cashback = purchase × merchant rate, paid from merchant's USDC vault
```

## How it works

1. **The merchant funds cashback.** Each merchant has a USDC vault owned by the program. The merchant (or anyone else) tops it up with `fund_merchant`, sets its own rate and per-transaction cap with `update_cashback`, and can take unused funds back with `withdraw_merchant`.
2. **The card transaction comes in.** The backend (the *keeper*) receives the card authorization or settlement event and works out which merchant and which user wallet it belongs to.
3. **The keeper picks the asset and the route.** If the merchant has a tokenized stock and a swap route exists, the asset is that stock. Otherwise it is the fallback asset (wSOL). The keeper gets a route from Jupiter and calls `process_cashback`.
4. **The program enforces the rules.** It computes the cashback, checks that the vault can pay it, runs the swap while signing as the merchant vault, and then checks the result:
   - the vault spent **no more than the cashback**
   - the user received **at least `min_amount_out`**
   - the output token is **either the merchant's stock or the fallback asset**
   - the vault has no delegate or close authority left behind
5. **A receipt is written.** It's a PDA keyed by the card transaction id. This makes every card transaction processable exactly once.

## Programs

| Program | Purpose |
| --- | --- |
| `programs/loyalty_buyback` | The core program |
| `programs/mock_swap` | Test-only stand-in for Jupiter: takes USDC and mints the output token. Used on localnet and in tests |

### Accounts (`loyalty_buyback`)

| Account | Seeds | Holds |
| --- | --- | --- |
| `Config` | `["config"]` | admin, keeper, USDC mint, fallback mint (wSOL), allowed swap program, pause flag |
| `Merchant` | `["merchant", merchant_id]` | merchant authority, stock mint (optional), cashback bps, per-tx cap, stats |
| vault | ATA(merchant PDA, USDC) | the merchant's pre-funded cashback |
| `Receipt` | `["receipt", card_tx_id]` | one per processed card transaction |

`merchant_id` and `card_tx_id` are 32-byte ids. Use a hash of the card network's merchant id and transaction id.

### Instructions

| Instruction | Signer | What it does |
| --- | --- | --- |
| `initialize_config` | admin | One-time setup |
| `update_config` | admin | Rotate admin or keeper, change the fallback mint or swap program, pause |
| `register_merchant` | admin | Onboard a merchant, create its vault, optionally map it to a stock token |
| `set_merchant_stock` | admin | Map or unmap the merchant's tokenized stock. Admin-only, so a merchant can't point cashback at an arbitrary token |
| `update_cashback` | merchant | Set the cashback rate (0–100%), per-tx cap and active flag |
| `fund_merchant` | anyone | Top up a merchant vault with USDC |
| `withdraw_merchant` | merchant | Withdraw unused USDC |
| `process_cashback` | keeper | **Core flow:** card tx → cashback → swap → user's wallet |

## Keeper integration (Jupiter)

`process_cashback(card_tx_id, purchase_amount, min_amount_out, swap_data)` forwards `swap_data` and its remaining accounts to the configured swap program. It signs as the merchant PDA. To build these from Jupiter:

- `GET /quote`: `inputMint = USDC`, `outputMint = stock mint` (or wSOL), `amount = cashback`
- `POST /swap-instructions`: `userPublicKey = merchant PDA`, `destinationTokenAccount = user's ATA for the output mint`, `wrapAndUnwrapSol = false`
- Put the swap instruction's `data` in `swap_data` and its accounts as remaining accounts, in the same order and **with `isSigner = false`**. The program marks the merchant PDA as signer itself.
- Use the quote's `otherAmountThreshold` as `min_amount_out`, and send it as a v0 transaction with Jupiter's address lookup tables.
- If no route exists for the stock, request a quote for wSOL and call again with `output_mint = fallback mint`.

`user_output_account` is the user's associated token account. The program creates it if needed, and the keeper pays the rent. Tokenized stocks such as xStocks are Token-2022 mints, so pass the Token-2022 program as `output_token_program` for those.

## Build & test

Requires Anchor 1.2.x and the Solana CLI. The tests run in [LiteSVM](https://github.com/LiteSVM/litesvm), which loads SBPF v2, so build with `--arch v2`:

```bash
anchor build --arch v2
cargo test -p loyalty_buyback
```

The tests cover: buying stock into the user's wallet (Token-2022 stock), SOL fallback both with and without a stock mapping, replay protection, the per-tx cap, overspending routes, slippage, unrelated output mints, an underfunded merchant, keeper-only processing, and merchant rate control and withdrawal.

## Deploy to devnet

```bash
npm install
npm run deploy:devnet                               # build, deploy both programs, initialize the demo setup
npm run simulate:devnet -- "Corner Coffee" 42.50    # simulate a card payment
```

`deploy:devnet` uses its own deployer key (`.keys/devnet-deployer.json`, gitignored) and always passes `--url devnet`, so it never touches your global Solana config or mainnet. It needs about 5 SOL: roughly 2.2 SOL is program rent, and the deploy briefly needs a buffer of the same size again. If the airdrop is rate-limited, fund the printed address at [faucet.solana.com](https://faucet.solana.com) and run it again, or point `DEPLOYER_KEYPAIR` at a funded key.

Jupiter and tokenized stocks exist only on mainnet, so the devnet setup uses test assets with fixed prices, all minted by `mock_swap`:

| Asset | Stands in for |
| --- | --- |
| `tUSDC` | USDC (6 decimals; the deployer can mint it) |
| `tSBUX`, `tNKE`, `tAAPL` | tokenized stocks (Token-2022, 8 decimals, like xStocks) |
| `tSOL` | wSOL (the fallback) |

It registers four demo merchants and funds each with 1,000 tUSDC:

| Merchant | Cashback | Buys |
| --- | --- | --- |
| Corner Coffee | 5% | tSBUX |
| Sneaker Store | 3% | tNKE |
| Apple Store | 2% | tAAPL |
| Local Bakery | 10% | tSOL (no stock → fallback) |

All addresses are written to `deployments/devnet.json`. The simulator sends cashback to `.keys/devnet-demo-user.json` by default. Pass a wallet address as the third argument to see the tokens show up in Phantom or another wallet.

## Security notes and limitations

- **The keeper is trusted** to report real card transactions and the right user wallet. The program limits the damage a compromised keeper can do: it can never spend more than a merchant's cashback per transaction, process a card transaction twice, or buy anything other than that merchant's stock or the fallback. It does choose the user wallet, though, so its key should live in a KMS or HSM, and merchants should keep only a float in their vaults.
- **The swap program is pinned** in `Config`. Only that program (Jupiter on mainnet) can be invoked with the vault's signature.
- **Initialize right after deploying.** `initialize_config` can be called by anyone until it has been called once, so deploy and initialize in one script.
- **SOL is delivered as wSOL** to the user's ATA. The wallet can unwrap it, or a later version can close the account to deliver native SOL.
- **Compliance:** tokenized equities are restricted in some jurisdictions. Check user eligibility in the backend before choosing the stock asset over the fallback.
