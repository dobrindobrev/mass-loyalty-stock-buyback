use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{Mint, TokenAccount, TokenInterface},
};

use crate::{constants::*, error::ErrorCode, state::*};

#[derive(Accounts)]
pub struct InitializeConfig<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(
        init,
        payer = admin,
        space = 8 + Config::INIT_SPACE,
        seeds = [CONFIG_SEED],
        bump
    )]
    pub config: Account<'info, Config>,
    pub usdc_mint: InterfaceAccount<'info, Mint>,
    pub system_program: Program<'info, System>,
}

pub fn handle_initialize_config(
    ctx: Context<InitializeConfig>,
    keeper: Pubkey,
    fallback_mint: Pubkey,
    swap_program: Pubkey,
) -> Result<()> {
    ctx.accounts.config.set_inner(Config {
        admin: ctx.accounts.admin.key(),
        keeper,
        usdc_mint: ctx.accounts.usdc_mint.key(),
        fallback_mint,
        swap_program,
        paused: false,
        bump: ctx.bumps.config,
    });
    Ok(())
}

#[derive(Accounts)]
pub struct UpdateConfig<'info> {
    pub admin: Signer<'info>,
    #[account(
        mut,
        seeds = [CONFIG_SEED],
        bump = config.bump,
        has_one = admin @ ErrorCode::Unauthorized
    )]
    pub config: Account<'info, Config>,
}

pub fn handle_update_config(
    ctx: Context<UpdateConfig>,
    new_admin: Option<Pubkey>,
    keeper: Option<Pubkey>,
    fallback_mint: Option<Pubkey>,
    swap_program: Option<Pubkey>,
    paused: Option<bool>,
) -> Result<()> {
    let config = &mut ctx.accounts.config;
    if let Some(v) = new_admin {
        config.admin = v;
    }
    if let Some(v) = keeper {
        config.keeper = v;
    }
    if let Some(v) = fallback_mint {
        config.fallback_mint = v;
    }
    if let Some(v) = swap_program {
        config.swap_program = v;
    }
    if let Some(v) = paused {
        config.paused = v;
    }
    Ok(())
}

#[derive(Accounts)]
#[instruction(merchant_id: [u8; 32])]
pub struct RegisterMerchant<'info> {
    #[account(mut)]
    pub admin: Signer<'info>,
    #[account(
        seeds = [CONFIG_SEED],
        bump = config.bump,
        has_one = admin @ ErrorCode::Unauthorized,
        has_one = usdc_mint
    )]
    pub config: Account<'info, Config>,
    #[account(
        init,
        payer = admin,
        space = 8 + Merchant::INIT_SPACE,
        seeds = [MERCHANT_SEED, merchant_id.as_ref()],
        bump
    )]
    pub merchant: Account<'info, Merchant>,
    pub usdc_mint: InterfaceAccount<'info, Mint>,
    /// USDC the merchant pre-funds; owned by the merchant PDA.
    #[account(
        init,
        payer = admin,
        associated_token::mint = usdc_mint,
        associated_token::authority = merchant,
        associated_token::token_program = token_program
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,
    pub token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

pub fn handle_register_merchant(
    ctx: Context<RegisterMerchant>,
    merchant_id: [u8; 32],
    authority: Pubkey,
    stock_mint: Option<Pubkey>,
    cashback_bps: u16,
    max_cashback_per_tx: u64,
) -> Result<()> {
    require!(
        cashback_bps <= MAX_CASHBACK_BPS,
        ErrorCode::InvalidCashbackRate
    );
    ctx.accounts.merchant.set_inner(Merchant {
        merchant_id,
        authority,
        stock_mint,
        cashback_bps,
        max_cashback_per_tx,
        active: true,
        total_usdc_spent: 0,
        total_transactions: 0,
        bump: ctx.bumps.merchant,
    });
    Ok(())
}

#[derive(Accounts)]
pub struct SetMerchantStock<'info> {
    pub admin: Signer<'info>,
    #[account(
        seeds = [CONFIG_SEED],
        bump = config.bump,
        has_one = admin @ ErrorCode::Unauthorized
    )]
    pub config: Account<'info, Config>,
    #[account(
        mut,
        seeds = [MERCHANT_SEED, merchant.merchant_id.as_ref()],
        bump = merchant.bump
    )]
    pub merchant: Account<'info, Merchant>,
}

/// Admin-only so a merchant cannot point its cashback at an arbitrary token.
pub fn handle_set_merchant_stock(
    ctx: Context<SetMerchantStock>,
    stock_mint: Option<Pubkey>,
) -> Result<()> {
    ctx.accounts.merchant.stock_mint = stock_mint;
    Ok(())
}
