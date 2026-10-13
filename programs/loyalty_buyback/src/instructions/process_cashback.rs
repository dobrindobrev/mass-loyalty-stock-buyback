use anchor_lang::{
    prelude::*,
    solana_program::{
        instruction::{AccountMeta, Instruction},
        program::invoke_signed,
    },
};
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{Mint, TokenAccount, TokenInterface},
};

use crate::{constants::*, error::ErrorCode, state::*};

#[derive(Accounts)]
#[instruction(card_tx_id: [u8; 32])]
pub struct ProcessCashback<'info> {
    #[account(mut)]
    pub keeper: Signer<'info>,
    #[account(
        seeds = [CONFIG_SEED],
        bump = config.bump,
        has_one = keeper @ ErrorCode::Unauthorized,
        has_one = usdc_mint,
        constraint = !config.paused @ ErrorCode::Paused
    )]
    pub config: Box<Account<'info, Config>>,
    #[account(
        mut,
        seeds = [MERCHANT_SEED, merchant.merchant_id.as_ref()],
        bump = merchant.bump,
        constraint = merchant.active @ ErrorCode::MerchantInactive
    )]
    pub merchant: Box<Account<'info, Merchant>>,
    pub usdc_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        mut,
        associated_token::mint = usdc_mint,
        associated_token::authority = merchant,
        associated_token::token_program = usdc_token_program
    )]
    pub vault: Box<InterfaceAccount<'info, TokenAccount>>,
    /// CHECK: the cardholder's wallet; only used as the owner of the output token account.
    pub user: UncheckedAccount<'info>,
    /// Either the merchant's stock token or the fallback asset; checked in the handler.
    pub output_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(
        init_if_needed,
        payer = keeper,
        associated_token::mint = output_mint,
        associated_token::authority = user,
        associated_token::token_program = output_token_program
    )]
    pub user_output_account: Box<InterfaceAccount<'info, TokenAccount>>,
    /// Exists once per card transaction, so a replayed transaction fails here.
    #[account(
        init,
        payer = keeper,
        space = 8 + Receipt::INIT_SPACE,
        seeds = [RECEIPT_SEED, card_tx_id.as_ref()],
        bump
    )]
    pub receipt: Box<Account<'info, Receipt>>,
    /// CHECK: pinned to the swap program in config.
    #[account(address = config.swap_program)]
    pub swap_program: UncheckedAccount<'info>,
    pub usdc_token_program: Interface<'info, TokenInterface>,
    pub output_token_program: Interface<'info, TokenInterface>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

/// Core flow: card transaction -> merchant-funded USDC cashback -> swap into the
/// merchant's tokenized stock (or the fallback asset) -> delivered to the user's wallet.
///
/// The keeper builds the swap route off-chain (e.g. a Jupiter route with the merchant
/// PDA as the user and `user_output_account` as destination) and passes its instruction
/// data in `swap_data` and its accounts as remaining accounts. The program signs for the
/// merchant vault and checks the result, so a bad route cannot spend more than the
/// cashback or deliver less than `min_amount_out`.
pub fn handle_process_cashback<'info>(
    ctx: Context<'info, ProcessCashback<'info>>,
    card_tx_id: [u8; 32],
    purchase_amount: u64,
    min_amount_out: u64,
    swap_data: Vec<u8>,
) -> Result<()> {
    require!(purchase_amount > 0, ErrorCode::ZeroAmount);
    require!(min_amount_out > 0, ErrorCode::ZeroAmount);

    let merchant = &ctx.accounts.merchant;
    let cashback = compute_cashback(
        purchase_amount,
        merchant.cashback_bps,
        merchant.max_cashback_per_tx,
    )?;
    require!(cashback > 0, ErrorCode::CashbackTooSmall);
    require!(
        ctx.accounts.vault.amount >= cashback,
        ErrorCode::InsufficientMerchantFunds
    );

    let output_mint = ctx.accounts.output_mint.key();
    let kind = if merchant.stock_mint == Some(output_mint) {
        PurchaseKind::Stock
    } else if output_mint == ctx.accounts.config.fallback_mint {
        PurchaseKind::Fallback
    } else {
        return err!(ErrorCode::InvalidOutputMint);
    };

    let vault_before = ctx.accounts.vault.amount;
    let user_before = ctx.accounts.user_output_account.amount;

    // Forward the route to the swap program, signing as the merchant PDA.
    let merchant_key = merchant.key();
    let metas: Vec<AccountMeta> = ctx
        .remaining_accounts
        .iter()
        .map(|a| AccountMeta {
            pubkey: *a.key,
            is_signer: a.is_signer || *a.key == merchant_key,
            is_writable: a.is_writable,
        })
        .collect();
    let ix = Instruction {
        program_id: ctx.accounts.swap_program.key(),
        accounts: metas,
        data: swap_data,
    };
    let mut infos = ctx.remaining_accounts.to_vec();
    infos.push(ctx.accounts.swap_program.to_account_info());
    let merchant_id = merchant.merchant_id;
    let seeds: &[&[u8]] = &[MERCHANT_SEED, merchant_id.as_ref(), &[merchant.bump]];
    invoke_signed(&ix, &infos, &[seeds])?;

    // Verify what the swap actually did.
    ctx.accounts.vault.reload()?;
    ctx.accounts.user_output_account.reload()?;
    let vault = &ctx.accounts.vault;
    require!(
        vault.owner == merchant_key && vault.delegate.is_none() && vault.close_authority.is_none(),
        ErrorCode::VaultTampered
    );
    let usdc_spent = vault_before
        .checked_sub(vault.amount)
        .ok_or(ErrorCode::VaultTampered)?;
    require!(usdc_spent <= cashback, ErrorCode::SwapOverspent);
    let amount_out = ctx
        .accounts
        .user_output_account
        .amount
        .checked_sub(user_before)
        .ok_or(ErrorCode::MathOverflow)?;
    require!(amount_out >= min_amount_out, ErrorCode::SlippageExceeded);

    let user = ctx.accounts.user.key();
    let now = Clock::get()?.unix_timestamp;
    ctx.accounts.receipt.set_inner(Receipt {
        card_tx_id,
        merchant: merchant_key,
        user,
        purchase_amount,
        cashback_amount: cashback,
        usdc_spent,
        output_mint,
        amount_out,
        kind,
        timestamp: now,
        bump: ctx.bumps.receipt,
    });

    let merchant = &mut ctx.accounts.merchant;
    merchant.total_usdc_spent = merchant
        .total_usdc_spent
        .checked_add(usdc_spent)
        .ok_or(ErrorCode::MathOverflow)?;
    merchant.total_transactions = merchant
        .total_transactions
        .checked_add(1)
        .ok_or(ErrorCode::MathOverflow)?;

    emit!(CashbackProcessed {
        card_tx_id,
        merchant: merchant_key,
        user,
        cashback_amount: cashback,
        usdc_spent,
        output_mint,
        amount_out,
        kind,
    });
    Ok(())
}

pub fn compute_cashback(purchase_amount: u64, bps: u16, cap: u64) -> Result<u64> {
    let raw = (purchase_amount as u128)
        .checked_mul(bps as u128)
        .ok_or(ErrorCode::MathOverflow)?
        / BPS_DENOMINATOR;
    let raw = u64::try_from(raw).map_err(|_| ErrorCode::MathOverflow)?;
    Ok(if cap > 0 { raw.min(cap) } else { raw })
}
