use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, system_program},
        AccountDeserialize, InstructionData, ToAccountMetas,
    },
    anchor_spl::token_interface::TokenAccount,
    litesvm::LiteSVM,
    litesvm_token::{CreateAssociatedTokenAccount, CreateMint, MintTo},
    loyalty_buyback::{
        Merchant, PurchaseKind, Receipt, CONFIG_SEED, MERCHANT_SEED, RECEIPT_SEED,
    },
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
    spl_associated_token_account_interface::address::get_associated_token_address_with_program_id as ata,
};

const TOKEN: Pubkey = anchor_spl::token::ID;
const TOKEN_2022: Pubkey = anchor_spl::token_2022::ID;
const ATA_PROGRAM: Pubkey = anchor_spl::associated_token::ID;
const USDC: u64 = 1_000_000;

struct Env {
    svm: LiteSVM,
    admin: Keypair,
    keeper: Keypair,
    merchant_authority: Keypair,
    user: Keypair,
    usdc_mint: Pubkey,
    /// Tokenized stock (Token-2022, like xStocks).
    stock_mint: Pubkey,
    /// Stand-in for wSOL.
    fallback_mint: Pubkey,
    pool_authority: Pubkey,
    pool_usdc: Pubkey,
}

fn config_pda() -> Pubkey {
    Pubkey::find_program_address(&[CONFIG_SEED], &loyalty_buyback::id()).0
}

fn merchant_pda(id: &[u8; 32]) -> Pubkey {
    Pubkey::find_program_address(&[MERCHANT_SEED, id], &loyalty_buyback::id()).0
}

fn receipt_pda(card_tx_id: &[u8; 32]) -> Pubkey {
    Pubkey::find_program_address(&[RECEIPT_SEED, card_tx_id], &loyalty_buyback::id()).0
}

fn send(svm: &mut LiteSVM, ixs: &[Instruction], payer: &Keypair, signers: &[&Keypair]) -> Result<(), String> {
    svm.expire_blockhash();
    let msg = Message::new_with_blockhash(ixs, Some(&payer.pubkey()), &svm.latest_blockhash());
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), signers).unwrap();
    svm.send_transaction(tx)
        .map(|_| ())
        .map_err(|e| e.meta.logs.join("\n"))
}

fn expect_err(res: Result<(), String>, code: &str) {
    let logs = res.expect_err("transaction should have failed");
    assert!(logs.contains(code), "expected `{code}` in logs:\n{logs}");
}

fn token_amount(svm: &LiteSVM, address: &Pubkey) -> u64 {
    let acc = svm.get_account(address).unwrap();
    TokenAccount::try_deserialize(&mut acc.data.as_slice()).unwrap().amount
}

fn load<T: AccountDeserialize>(svm: &LiteSVM, address: &Pubkey) -> T {
    let acc = svm.get_account(address).unwrap();
    T::try_deserialize(&mut acc.data.as_slice()).unwrap()
}

fn setup() -> Env {
    let mut svm = LiteSVM::new();
    svm.add_program(
        loyalty_buyback::id(),
        include_bytes!(concat!(env!("CARGO_TARGET_TMPDIR"), "/../deploy/loyalty_buyback.so")),
    )
    .unwrap();
    svm.add_program(
        mock_swap::id(),
        include_bytes!(concat!(env!("CARGO_TARGET_TMPDIR"), "/../deploy/mock_swap.so")),
    )
    .unwrap();

    let admin = Keypair::new();
    let keeper = Keypair::new();
    let merchant_authority = Keypair::new();
    let user = Keypair::new();
    for kp in [&admin, &keeper, &merchant_authority] {
        svm.airdrop(&kp.pubkey(), 10_000_000_000).unwrap();
    }

    let pool_authority =
        Pubkey::find_program_address(&[mock_swap::POOL_SEED], &mock_swap::id()).0;

    let usdc_mint = CreateMint::new(&mut svm, &admin)
        .authority(&admin.pubkey())
        .decimals(6)
        .token_program_id(&TOKEN)
        .send()
        .unwrap();
    let stock_mint = CreateMint::new(&mut svm, &admin)
        .authority(&pool_authority)
        .decimals(8)
        .token_program_id(&TOKEN_2022)
        .send()
        .unwrap();
    let fallback_mint = CreateMint::new(&mut svm, &admin)
        .authority(&pool_authority)
        .decimals(9)
        .token_program_id(&TOKEN)
        .send()
        .unwrap();
    let pool_usdc = CreateAssociatedTokenAccount::new(&mut svm, &admin, &usdc_mint)
        .owner(&pool_authority)
        .token_program_id(&TOKEN)
        .send()
        .unwrap();

    // Merchant tops up from its own USDC account.
    let merchant_usdc = CreateAssociatedTokenAccount::new(&mut svm, &admin, &usdc_mint)
        .owner(&merchant_authority.pubkey())
        .token_program_id(&TOKEN)
        .send()
        .unwrap();
    MintTo::new(&mut svm, &admin, &usdc_mint, &merchant_usdc, 1_000 * USDC)
        .owner(&admin)
        .token_program_id(&TOKEN)
        .send()
        .unwrap();

    let mut env = Env {
        svm,
        admin,
        keeper,
        merchant_authority,
        user,
        usdc_mint,
        stock_mint,
        fallback_mint,
        pool_authority,
        pool_usdc,
    };

    let ix = Instruction::new_with_bytes(
        loyalty_buyback::id(),
        &loyalty_buyback::instruction::InitializeConfig {
            keeper: env.keeper.pubkey(),
            fallback_mint: env.fallback_mint,
            swap_program: mock_swap::id(),
        }
        .data(),
        loyalty_buyback::accounts::InitializeConfig {
            admin: env.admin.pubkey(),
            config: config_pda(),
            usdc_mint: env.usdc_mint,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    );
    send(&mut env.svm, &[ix], &env.admin, &[&env.admin]).unwrap();
    env
}

/// Registers a merchant (5% cashback by default) and funds its vault.
fn add_merchant(env: &mut Env, id: [u8; 32], stock: Option<Pubkey>, bps: u16, cap: u64, fund: u64) -> Pubkey {
    let merchant = merchant_pda(&id);
    let vault = ata(&merchant, &env.usdc_mint, &TOKEN);
    let register = Instruction::new_with_bytes(
        loyalty_buyback::id(),
        &loyalty_buyback::instruction::RegisterMerchant {
            merchant_id: id,
            authority: env.merchant_authority.pubkey(),
            stock_mint: stock,
            cashback_bps: bps,
            max_cashback_per_tx: cap,
        }
        .data(),
        loyalty_buyback::accounts::RegisterMerchant {
            admin: env.admin.pubkey(),
            config: config_pda(),
            merchant,
            usdc_mint: env.usdc_mint,
            vault,
            token_program: TOKEN,
            associated_token_program: ATA_PROGRAM,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    );
    send(&mut env.svm, &[register], &env.admin, &[&env.admin]).unwrap();

    if fund > 0 {
        let fund_ix = Instruction::new_with_bytes(
            loyalty_buyback::id(),
            &loyalty_buyback::instruction::FundMerchant { amount: fund }.data(),
            loyalty_buyback::accounts::FundMerchant {
                funder: env.merchant_authority.pubkey(),
                config: config_pda(),
                merchant,
                usdc_mint: env.usdc_mint,
                funder_token_account: ata(&env.merchant_authority.pubkey(), &env.usdc_mint, &TOKEN),
                vault,
                token_program: TOKEN,
            }
            .to_account_metas(None),
        );
        let ma = env.merchant_authority.insecure_clone();
        send(&mut env.svm, &[fund_ix], &ma, &[&ma]).unwrap();
    }
    merchant
}

struct Cashback {
    merchant_id: [u8; 32],
    card_tx_id: [u8; 32],
    purchase_amount: u64,
    output_mint: Pubkey,
    /// What the (mock) route spends and returns.
    swap_in: u64,
    swap_out: u64,
    min_amount_out: u64,
}

fn process_ix(env: &Env, c: &Cashback, keeper: &Pubkey) -> Instruction {
    let merchant = merchant_pda(&c.merchant_id);
    let vault = ata(&merchant, &env.usdc_mint, &TOKEN);
    let output_program = if c.output_mint == env.stock_mint { TOKEN_2022 } else { TOKEN };
    let user_output = ata(&env.user.pubkey(), &c.output_mint, &output_program);

    let mut accounts = loyalty_buyback::accounts::ProcessCashback {
        keeper: *keeper,
        config: config_pda(),
        merchant,
        usdc_mint: env.usdc_mint,
        vault,
        user: env.user.pubkey(),
        output_mint: c.output_mint,
        user_output_account: user_output,
        receipt: receipt_pda(&c.card_tx_id),
        swap_program: mock_swap::id(),
        usdc_token_program: TOKEN,
        output_token_program: output_program,
        associated_token_program: ATA_PROGRAM,
        system_program: system_program::ID,
    }
    .to_account_metas(None);

    // The route the keeper would get from Jupiter; here the mock's accounts.
    let route = mock_swap::accounts::Swap {
        user_authority: merchant,
        source: vault,
        input_mint: env.usdc_mint,
        pool_input: env.pool_usdc,
        output_mint: c.output_mint,
        destination: user_output,
        pool_authority: env.pool_authority,
        input_token_program: TOKEN,
        output_token_program: output_program,
    }
    .to_account_metas(None)
    .into_iter()
    .map(|mut m| {
        m.is_signer = false; // the program signs for the merchant PDA itself
        m
    });
    accounts.extend(route);

    Instruction::new_with_bytes(
        loyalty_buyback::id(),
        &loyalty_buyback::instruction::ProcessCashback {
            card_tx_id: c.card_tx_id,
            purchase_amount: c.purchase_amount,
            min_amount_out: c.min_amount_out,
            swap_data: mock_swap::instruction::Swap {
                amount_in: c.swap_in,
                amount_out: c.swap_out,
            }
            .data(),
        }
        .data(),
        accounts,
    )
}

fn process(env: &mut Env, c: &Cashback) -> Result<(), String> {
    let ix = process_ix(env, c, &env.keeper.pubkey());
    let keeper = env.keeper.insecure_clone();
    send(&mut env.svm, &[ix], &keeper, &[&keeper])
}

fn stock_purchase(env: &Env, tx: u8) -> Cashback {
    Cashback {
        merchant_id: [1; 32],
        card_tx_id: [tx; 32],
        purchase_amount: 40 * USDC, // 5% -> 2 USDC cashback
        output_mint: env.stock_mint,
        swap_in: 2 * USDC,
        swap_out: 1_000_000, // 0.01 share at 8 decimals
        min_amount_out: 990_000,
    }
}

#[test]
fn card_purchase_buys_merchant_stock_into_user_wallet() {
    let mut env = setup();
    let stock = env.stock_mint;
    let merchant = add_merchant(&mut env, [1; 32], Some(stock), 500, 0, 100 * USDC);
    let c = stock_purchase(&env, 7);
    process(&mut env, &c).unwrap();

    let user_stock = ata(&env.user.pubkey(), &env.stock_mint, &TOKEN_2022);
    assert_eq!(token_amount(&env.svm, &user_stock), 1_000_000);
    assert_eq!(token_amount(&env.svm, &ata(&merchant, &env.usdc_mint, &TOKEN)), 98 * USDC);

    let receipt: Receipt = load(&env.svm, &receipt_pda(&c.card_tx_id));
    assert_eq!(receipt.kind, PurchaseKind::Stock);
    assert_eq!(receipt.cashback_amount, 2 * USDC);
    assert_eq!(receipt.usdc_spent, 2 * USDC);
    assert_eq!(receipt.amount_out, 1_000_000);
    assert_eq!(receipt.user, env.user.pubkey());

    let m: Merchant = load(&env.svm, &merchant);
    assert_eq!(m.total_transactions, 1);
    assert_eq!(m.total_usdc_spent, 2 * USDC);

    // A second purchase accumulates in the same wallet.
    let c2 = stock_purchase(&env, 8);
    process(&mut env, &c2).unwrap();
    assert_eq!(token_amount(&env.svm, &user_stock), 2_000_000);
}

#[test]
fn merchant_without_stock_falls_back_to_sol() {
    let mut env = setup();
    add_merchant(&mut env, [2; 32], None, 1_000, 0, 100 * USDC);
    let c = Cashback {
        merchant_id: [2; 32],
        card_tx_id: [9; 32],
        purchase_amount: 10 * USDC, // 10% -> 1 USDC
        output_mint: env.fallback_mint,
        swap_in: USDC,
        swap_out: 5_000_000, // 0.005 SOL
        min_amount_out: 5_000_000,
    };
    process(&mut env, &c).unwrap();

    let user_sol = ata(&env.user.pubkey(), &env.fallback_mint, &TOKEN);
    assert_eq!(token_amount(&env.svm, &user_sol), 5_000_000);
    let receipt: Receipt = load(&env.svm, &receipt_pda(&c.card_tx_id));
    assert_eq!(receipt.kind, PurchaseKind::Fallback);
}

#[test]
fn merchant_with_stock_can_fall_back_when_no_route() {
    let mut env = setup();
    let stock = env.stock_mint;
    add_merchant(&mut env, [1; 32], Some(stock), 500, 0, 100 * USDC);
    let mut c = stock_purchase(&env, 3);
    c.output_mint = env.fallback_mint;
    process(&mut env, &c).unwrap();
    let receipt: Receipt = load(&env.svm, &receipt_pda(&c.card_tx_id));
    assert_eq!(receipt.kind, PurchaseKind::Fallback);
}

#[test]
fn same_card_transaction_cannot_be_processed_twice() {
    let mut env = setup();
    let stock = env.stock_mint;
    add_merchant(&mut env, [1; 32], Some(stock), 500, 0, 100 * USDC);
    let c = stock_purchase(&env, 4);
    process(&mut env, &c).unwrap();
    expect_err(process(&mut env, &c), "already in use");
}

#[test]
fn per_transaction_cap_limits_cashback() {
    let mut env = setup();
    let stock = env.stock_mint;
    add_merchant(&mut env, [1; 32], Some(stock), 500, USDC / 2, 100 * USDC);
    let mut c = stock_purchase(&env, 5); // 5% of 40 = 2 USDC, capped to 0.5
    expect_err(process(&mut env, &c), "SwapOverspent");
    c.swap_in = USDC / 2;
    process(&mut env, &c).unwrap();
    let receipt: Receipt = load(&env.svm, &receipt_pda(&c.card_tx_id));
    assert_eq!(receipt.cashback_amount, USDC / 2);
}

#[test]
fn rejects_route_that_overspends_vault() {
    let mut env = setup();
    let stock = env.stock_mint;
    add_merchant(&mut env, [1; 32], Some(stock), 500, 0, 100 * USDC);
    let mut c = stock_purchase(&env, 6);
    c.swap_in = 50 * USDC;
    expect_err(process(&mut env, &c), "SwapOverspent");
}

#[test]
fn rejects_route_below_min_amount_out() {
    let mut env = setup();
    let stock = env.stock_mint;
    add_merchant(&mut env, [1; 32], Some(stock), 500, 0, 100 * USDC);
    let mut c = stock_purchase(&env, 6);
    c.swap_out = 1;
    expect_err(process(&mut env, &c), "SlippageExceeded");
}

#[test]
fn rejects_unrelated_output_mint() {
    let mut env = setup();
    add_merchant(&mut env, [1; 32], None, 500, 0, 100 * USDC);
    let mut c = stock_purchase(&env, 6);
    c.output_mint = env.stock_mint; // merchant has no stock configured
    expect_err(process(&mut env, &c), "InvalidOutputMint");
}

#[test]
fn rejects_when_merchant_has_not_funded_enough() {
    let mut env = setup();
    let stock = env.stock_mint;
    add_merchant(&mut env, [1; 32], Some(stock), 500, 0, USDC);
    let c = stock_purchase(&env, 6); // needs 2 USDC
    expect_err(process(&mut env, &c), "InsufficientMerchantFunds");
}

#[test]
fn only_keeper_can_process() {
    let mut env = setup();
    let stock = env.stock_mint;
    add_merchant(&mut env, [1; 32], Some(stock), 500, 0, 100 * USDC);
    let c = stock_purchase(&env, 6);
    let intruder = Keypair::new();
    env.svm.airdrop(&intruder.pubkey(), 1_000_000_000).unwrap();
    let ix = process_ix(&env, &c, &intruder.pubkey());
    expect_err(send(&mut env.svm, &[ix], &intruder, &[&intruder]), "Unauthorized");
}

#[test]
fn merchant_controls_rate_and_withdraws_unused_funds() {
    let mut env = setup();
    let stock = env.stock_mint;
    let merchant = add_merchant(&mut env, [1; 32], Some(stock), 500, 0, 100 * USDC);
    let ma = env.merchant_authority.insecure_clone();

    let update = |authority: &Pubkey| {
        Instruction::new_with_bytes(
            loyalty_buyback::id(),
            &loyalty_buyback::instruction::UpdateCashback {
                cashback_bps: 1_000,
                max_cashback_per_tx: 0,
                active: true,
            }
            .data(),
            loyalty_buyback::accounts::UpdateCashback { authority: *authority, merchant }
                .to_account_metas(None),
        )
    };
    let admin = env.admin.insecure_clone();
    expect_err(send(&mut env.svm, &[update(&admin.pubkey())], &admin, &[&admin]), "Unauthorized");
    send(&mut env.svm, &[update(&ma.pubkey())], &ma, &[&ma]).unwrap();
    assert_eq!(load::<Merchant>(&env.svm, &merchant).cashback_bps, 1_000);

    let withdraw = Instruction::new_with_bytes(
        loyalty_buyback::id(),
        &loyalty_buyback::instruction::WithdrawMerchant { amount: 60 * USDC }.data(),
        loyalty_buyback::accounts::WithdrawMerchant {
            authority: ma.pubkey(),
            config: config_pda(),
            merchant,
            usdc_mint: env.usdc_mint,
            vault: ata(&merchant, &env.usdc_mint, &TOKEN),
            destination: ata(&ma.pubkey(), &env.usdc_mint, &TOKEN),
            token_program: TOKEN,
        }
        .to_account_metas(None),
    );
    send(&mut env.svm, &[withdraw], &ma, &[&ma]).unwrap();
    assert_eq!(token_amount(&env.svm, &ata(&merchant, &env.usdc_mint, &TOKEN)), 40 * USDC);
}

#[test]
fn compute_cashback_math() {
    use loyalty_buyback::instructions::compute_cashback;
    assert_eq!(compute_cashback(40 * USDC, 500, 0).unwrap(), 2 * USDC);
    assert_eq!(compute_cashback(40 * USDC, 500, USDC).unwrap(), USDC);
    assert_eq!(compute_cashback(1, 500, 0).unwrap(), 0);
    assert_eq!(compute_cashback(u64::MAX, 10_000, 0).unwrap(), u64::MAX);
}
