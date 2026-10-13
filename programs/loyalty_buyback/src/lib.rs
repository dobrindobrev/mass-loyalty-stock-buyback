pub mod constants;
pub mod error;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("H1Q7cR9NxNGR556QagHS3aDTRaDdm8MGz6ZZEh27J38a");

#[program]
pub mod loyalty_buyback {
    use super::*;

    pub fn initialize_config(
        ctx: Context<InitializeConfig>,
        keeper: Pubkey,
        fallback_mint: Pubkey,
        swap_program: Pubkey,
    ) -> Result<()> {
        crate::instructions::admin::handle_initialize_config(ctx, keeper, fallback_mint, swap_program)
    }

    pub fn update_config(
        ctx: Context<UpdateConfig>,
        new_admin: Option<Pubkey>,
        keeper: Option<Pubkey>,
        fallback_mint: Option<Pubkey>,
        swap_program: Option<Pubkey>,
        paused: Option<bool>,
    ) -> Result<()> {
        crate::instructions::admin::handle_update_config(
            ctx,
            new_admin,
            keeper,
            fallback_mint,
            swap_program,
            paused,
        )
    }

    pub fn register_merchant(
        ctx: Context<RegisterMerchant>,
        merchant_id: [u8; 32],
        authority: Pubkey,
        stock_mint: Option<Pubkey>,
        cashback_bps: u16,
        max_cashback_per_tx: u64,
    ) -> Result<()> {
        crate::instructions::admin::handle_register_merchant(
            ctx,
            merchant_id,
            authority,
            stock_mint,
            cashback_bps,
            max_cashback_per_tx,
        )
    }

    pub fn set_merchant_stock(
        ctx: Context<SetMerchantStock>,
        stock_mint: Option<Pubkey>,
    ) -> Result<()> {
        crate::instructions::admin::handle_set_merchant_stock(ctx, stock_mint)
    }

    pub fn update_cashback(
        ctx: Context<UpdateCashback>,
        cashback_bps: u16,
        max_cashback_per_tx: u64,
        active: bool,
    ) -> Result<()> {
        crate::instructions::merchant::handle_update_cashback(
            ctx,
            cashback_bps,
            max_cashback_per_tx,
            active,
        )
    }

    pub fn fund_merchant(ctx: Context<FundMerchant>, amount: u64) -> Result<()> {
        crate::instructions::merchant::handle_fund_merchant(ctx, amount)
    }

    pub fn withdraw_merchant(ctx: Context<WithdrawMerchant>, amount: u64) -> Result<()> {
        crate::instructions::merchant::handle_withdraw_merchant(ctx, amount)
    }

    pub fn process_cashback<'info>(
        ctx: Context<'info, ProcessCashback<'info>>,
        card_tx_id: [u8; 32],
        purchase_amount: u64,
        min_amount_out: u64,
        swap_data: Vec<u8>,
    ) -> Result<()> {
        crate::instructions::process_cashback::handle_process_cashback(
            ctx,
            card_tx_id,
            purchase_amount,
            min_amount_out,
            swap_data,
        )
    }
}
