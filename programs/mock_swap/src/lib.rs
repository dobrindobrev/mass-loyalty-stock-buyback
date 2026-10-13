//! Test-only stand-in for a DEX aggregator (Jupiter on mainnet).
//! Pulls `amount_in` of the input token from the caller and mints `amount_out`
//! of the output token, whose mint authority is this program's pool PDA.
use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    mint_to, transfer_checked, Mint, MintTo, TokenAccount, TokenInterface, TransferChecked,
};

declare_id!("4oafyRYp9jTJyzgJB6XRyo3SCbuK9W1gkbhViUL5qqWj");

pub const POOL_SEED: &[u8] = b"pool";

#[program]
pub mod mock_swap {
    use super::*;

    pub fn swap(ctx: Context<Swap>, amount_in: u64, amount_out: u64) -> Result<()> {
        let a = &ctx.accounts;
        transfer_checked(
            CpiContext::new(
                a.input_token_program.key(),
                TransferChecked {
                    from: a.source.to_account_info(),
                    mint: a.input_mint.to_account_info(),
                    to: a.pool_input.to_account_info(),
                    authority: a.user_authority.to_account_info(),
                },
            ),
            amount_in,
            a.input_mint.decimals,
        )?;
        let bump = [ctx.bumps.pool_authority];
        let seeds: &[&[u8]] = &[POOL_SEED, &bump];
        mint_to(
            CpiContext::new_with_signer(
                a.output_token_program.key(),
                MintTo {
                    mint: a.output_mint.to_account_info(),
                    to: a.destination.to_account_info(),
                    authority: a.pool_authority.to_account_info(),
                },
                &[seeds],
            ),
            amount_out,
        )
    }
}

#[derive(Accounts)]
pub struct Swap<'info> {
    pub user_authority: Signer<'info>,
    #[account(mut)]
    pub source: InterfaceAccount<'info, TokenAccount>,
    pub input_mint: InterfaceAccount<'info, Mint>,
    #[account(mut, token::mint = input_mint, token::authority = pool_authority)]
    pub pool_input: InterfaceAccount<'info, TokenAccount>,
    #[account(mut)]
    pub output_mint: InterfaceAccount<'info, Mint>,
    #[account(mut)]
    pub destination: InterfaceAccount<'info, TokenAccount>,
    /// CHECK: PDA that owns the pool and is mint authority of output tokens.
    #[account(seeds = [POOL_SEED], bump)]
    pub pool_authority: UncheckedAccount<'info>,
    pub input_token_program: Interface<'info, TokenInterface>,
    pub output_token_program: Interface<'info, TokenInterface>,
}
