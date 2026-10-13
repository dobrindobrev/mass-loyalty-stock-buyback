use anchor_lang::prelude::*;

#[error_code]
pub enum ErrorCode {
    #[msg("Signer is not allowed to perform this action")]
    Unauthorized,
    #[msg("The program is paused")]
    Paused,
    #[msg("Merchant is not active")]
    MerchantInactive,
    #[msg("Cashback rate exceeds 100%")]
    InvalidCashbackRate,
    #[msg("Amount must be greater than zero")]
    ZeroAmount,
    #[msg("Computed cashback is zero")]
    CashbackTooSmall,
    #[msg("Merchant vault does not hold enough USDC for this cashback")]
    InsufficientMerchantFunds,
    #[msg("Output mint is neither the merchant's stock nor the fallback asset")]
    InvalidOutputMint,
    #[msg("Swap spent more USDC than the cashback amount")]
    SwapOverspent,
    #[msg("Swap delivered less than the minimum amount out")]
    SlippageExceeded,
    #[msg("Swap left the merchant vault in an unsafe state")]
    VaultTampered,
    #[msg("Arithmetic overflow")]
    MathOverflow,
}
