use anchor_lang::prelude::*;

#[constant]
pub const CONFIG_SEED: &[u8] = b"config";

#[constant]
pub const MERCHANT_SEED: &[u8] = b"merchant";

#[constant]
pub const RECEIPT_SEED: &[u8] = b"receipt";

/// 100% in basis points. Merchants may set any rate up to this.
#[constant]
pub const MAX_CASHBACK_BPS: u16 = 10_000;

pub const BPS_DENOMINATOR: u128 = 10_000;
