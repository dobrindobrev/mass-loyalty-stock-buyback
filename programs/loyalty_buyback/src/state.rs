use anchor_lang::prelude::*;

/// Global settings, one per deployment.
#[account]
#[derive(InitSpace)]
pub struct Config {
    /// Platform admin: onboards merchants, maps them to stock tokens, rotates keys.
    pub admin: Pubkey,
    /// Backend signer that reports card transactions and executes buybacks.
    pub keeper: Pubkey,
    /// Mint that cashback is funded and paid in (USDC).
    pub usdc_mint: Pubkey,
    /// Asset bought when the merchant has no tokenized stock or no route exists (wSOL).
    pub fallback_mint: Pubkey,
    /// The only program allowed to execute swaps (Jupiter on mainnet, mock on localnet).
    pub swap_program: Pubkey,
    pub paused: bool,
    pub bump: u8,
}

/// A brand that funds cashback for its customers.
#[account]
#[derive(InitSpace)]
pub struct Merchant {
    /// Off-chain merchant identifier (e.g. hash of the card network merchant ID).
    pub merchant_id: [u8; 32],
    /// Merchant-side key: sets the cashback rate and withdraws unused funds.
    pub authority: Pubkey,
    /// Tokenized stock of this company, if one exists.
    pub stock_mint: Option<Pubkey>,
    /// Cashback as basis points of the card purchase amount.
    pub cashback_bps: u16,
    /// Upper bound per card transaction in USDC base units (0 = no cap).
    pub max_cashback_per_tx: u64,
    pub active: bool,
    pub total_usdc_spent: u64,
    pub total_transactions: u64,
    pub bump: u8,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, InitSpace, Debug)]
pub enum PurchaseKind {
    Stock,
    Fallback,
}

/// One per card transaction. Its PDA (seeded by the card tx id) makes processing idempotent.
#[account]
#[derive(InitSpace)]
pub struct Receipt {
    pub card_tx_id: [u8; 32],
    pub merchant: Pubkey,
    pub user: Pubkey,
    pub purchase_amount: u64,
    pub cashback_amount: u64,
    pub usdc_spent: u64,
    pub output_mint: Pubkey,
    pub amount_out: u64,
    pub kind: PurchaseKind,
    pub timestamp: i64,
    pub bump: u8,
}

#[event]
pub struct CashbackProcessed {
    pub card_tx_id: [u8; 32],
    pub merchant: Pubkey,
    pub user: Pubkey,
    pub cashback_amount: u64,
    pub usdc_spent: u64,
    pub output_mint: Pubkey,
    pub amount_out: u64,
    pub kind: PurchaseKind,
}
