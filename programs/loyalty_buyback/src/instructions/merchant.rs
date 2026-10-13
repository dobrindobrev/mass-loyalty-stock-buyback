use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked,
};

use crate::{constants::*, error::ErrorCode, state::*};

#[derive(Accounts)]
pub struct UpdateCashback<'info> {
    pub authority: Signer<'info>,
    #[account(
        mut,
        seeds = [MERCHANT_SEED, merchant.merchant_id.as_ref()],
        bump = merchant.bump,
        has_one = authority @ ErrorCode::Unauthorized
    )]
    pub merchant: Account<'info, Merchant>,
}

/// Merchants set their own cashback rate and cap, and can pause their programme.
pub fn handle_update_cashback(
    ctx: Context<UpdateCashback>,
    cashback_bps: u16,
    max_cashback_per_tx: u64,
    active: bool,
) -> Result<()> {
    require!(
        cashback_bps <= MAX_CASHBACK_BPS,
        ErrorCode::InvalidCashbackRate
    );
    let merchant = &mut ctx.accounts.merchant;
    merchant.cashback_bps = cashback_bps;
    merchant.max_cashback_per_tx = max_cashback_per_tx;
    merchant.active = active;
    Ok(())
}

#[derive(Accounts)]
pub struct FundMerchant<'info> {
    pub funder: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump, has_one = usdc_mint)]
    pub config: Account<'info, Config>,
    #[account(
        seeds = [MERCHANT_SEED, merchant.merchant_id.as_ref()],
        bump = merchant.bump
    )]
    pub merchant: Account<'info, Merchant>,
    pub usdc_mint: InterfaceAccount<'info, Mint>,
    #[account(mut, token::mint = usdc_mint, token::authority = funder)]
    pub funder_token_account: InterfaceAccount<'info, TokenAccount>,
    #[account(
        mut,
        associated_token::mint = usdc_mint,
        associated_token::authority = merchant,
        associated_token::token_program = token_program
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,
    pub token_program: Interface<'info, TokenInterface>,
}

/// Anyone (usually the merchant) can top up a merchant's cashback balance.
pub fn handle_fund_merchant(ctx: Context<FundMerchant>, amount: u64) -> Result<()> {
    require!(amount > 0, ErrorCode::ZeroAmount);
    transfer_checked(
        CpiContext::new(
            ctx.accounts.token_program.key(),
            TransferChecked {
                from: ctx.accounts.funder_token_account.to_account_info(),
                mint: ctx.accounts.usdc_mint.to_account_info(),
                to: ctx.accounts.vault.to_account_info(),
                authority: ctx.accounts.funder.to_account_info(),
            },
        ),
        amount,
        ctx.accounts.usdc_mint.decimals,
    )
}

#[derive(Accounts)]
pub struct WithdrawMerchant<'info> {
    pub authority: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump, has_one = usdc_mint)]
    pub config: Account<'info, Config>,
    #[account(
        seeds = [MERCHANT_SEED, merchant.merchant_id.as_ref()],
        bump = merchant.bump,
        has_one = authority @ ErrorCode::Unauthorized
    )]
    pub merchant: Account<'info, Merchant>,
    pub usdc_mint: InterfaceAccount<'info, Mint>,
    #[account(
        mut,
        associated_token::mint = usdc_mint,
        associated_token::authority = merchant,
        associated_token::token_program = token_program
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,
    #[account(mut, token::mint = usdc_mint)]
    pub destination: InterfaceAccount<'info, TokenAccount>,
    pub token_program: Interface<'info, TokenInterface>,
}

/// The merchant can take back unused cashback funds at any time.
pub fn handle_withdraw_merchant(ctx: Context<WithdrawMerchant>, amount: u64) -> Result<()> {
    require!(amount > 0, ErrorCode::ZeroAmount);
    let merchant = &ctx.accounts.merchant;
    let seeds: &[&[u8]] = &[MERCHANT_SEED, merchant.merchant_id.as_ref(), &[merchant.bump]];
    transfer_checked(
        CpiContext::new_with_signer(
            ctx.accounts.token_program.key(),
            TransferChecked {
                from: ctx.accounts.vault.to_account_info(),
                mint: ctx.accounts.usdc_mint.to_account_info(),
                to: ctx.accounts.destination.to_account_info(),
                authority: merchant.to_account_info(),
            },
            &[seeds],
        ),
        amount,
        ctx.accounts.usdc_mint.decimals,
    )
}
