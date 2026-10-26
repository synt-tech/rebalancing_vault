//! Fuzz target 1: untrusted SwapPlan bytes → XDR decode → full harvest path.
//! The keeper-supplied `data` param is the strategy's least-trusted input:
//! `SwapPlan::from_xdr` and `validate_plan` must never panic, and `harvest`
//! must fail with a typed error rather than crash on adversarial plans.
#![no_main]
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::StellarAssetClient,
    vec, Address, Bytes, Env, IntoVal, Symbol, Vec,
};
use cl_aquarius_strategy::storage::Policy;
use cl_aquarius_strategy::{ClAquarius, ClAquariusClient};

const MOCK_WASM: &[u8] =
    include_bytes!("../../../target/wasm32v1-none/release/test_mocks.wasm");

fn policy(_e: &Env, venues: Vec<Address>) -> Policy {
    Policy {
        swap_allowed: venues,
        oracle_band_bps: 500,
        max_oracle_age: 600,
        max_slippage_bps: 300,
        max_loss_bps_per_window: 2000,
        loss_window_secs: 86_400,
        cooldown_secs: 0,
        range_min_bps: 50,
        range_max_bps: 50_000,
        tick_cap_bps: 10_000,
        perf_fee_bps: 1500,
        platform_split_bps: 5000,
        min_reward_swap: 1,
        max_reward_age: 3_600,
    }
}

fuzz_target!(|data: &[u8]| {
    // Note: `SwapPlan::from_xdr` outside a contract call escalates malformed
    // input to a host panic — inside `harvest` the same panic becomes a VM
    // trap and surfaces as a contract error. We fuzz through the contract
    // entry point, which is the untrusted-input surface that matters.
    let e = Env::default();
    e.cost_estimate().budget().reset_unlimited();
    e.mock_all_auths_allowing_non_root_auth();
    e.ledger().set_timestamp(1_700_000_000);

    let xlm = e.register_stellar_asset_contract_v2(Address::generate(&e)).address();
    let usdc = e.register_stellar_asset_contract_v2(Address::generate(&e)).address();
    let aqua = e.register_stellar_asset_contract_v2(Address::generate(&e)).address();
    let sqrtp: u128 = 125_260_000_000_000_000_000_000_000_000;
    let pool = e.register(MOCK_WASM, (xlm.clone(), usdc.clone(), sqrtp));
    let reward_pool = e.register(MOCK_WASM, (xlm.clone(), aqua.clone(), 1u128));
    let oracle = e.register(MOCK_WASM, (xlm.clone(), usdc.clone(), 1u128));
    let vault = e.register(MOCK_WASM, (xlm.clone(), usdc.clone(), 1u128));

    let manager = Address::generate(&e);
    let keeper = Address::generate(&e);
    let emergency = Address::generate(&e);
    let operator = Address::generate(&e);
    let treasury = Address::generate(&e);

    e.invoke_contract::<()>(&vault, &Symbol::new(&e, "init"), vec![
        &e, manager.into_val(&e), keeper.clone().into_val(&e),
        emergency.into_val(&e), operator.into_val(&e)]);
    e.invoke_contract::<()>(&vault, &Symbol::new(&e, "set_treasury"),
        vec![&e, treasury.clone().into_val(&e)]);
    for (tok, p) in [
        (usdc.clone(), 100_000_000_000_000i128),
        (xlm.clone(), 40_000_000_000_000i128),
        (aqua.clone(), 20_000_000_000_000i128),
    ] {
        e.invoke_contract::<()>(&oracle, &Symbol::new(&e, "set_price"),
            vec![&e, tok.into_val(&e), p.into_val(&e)]);
    }

    let pol = policy(&e, vec![&e, pool.clone(), reward_pool.clone()]);
    let init_args: Vec<soroban_sdk::Val> = vec![&e,
        pool.clone().into_val(&e), oracle.clone().into_val(&e),
        vault.clone().into_val(&e), treasury.into_val(&e),
        pol.into_val(&e), aqua.into_val(&e), reward_pool.into_val(&e)];
    let strat_addr = e.register(ClAquarius, (usdc.clone(), init_args));
    let strategy = ClAquariusClient::new(&e, &strat_addr);
    strategy.bind(&vault, &vault);

    StellarAssetClient::new(&e, &usdc).mint(&vault, &10_000_000_000);
    strategy.deposit(&10_000_000_000, &vault);

    // The call under test: any outcome is legal except a host panic —
    // contract Err and host errors are both acceptable results.
    let _ = strategy.try_harvest(&keeper, &Some(Bytes::from_slice(&e, data)));

    // Post-invariant: balance() must remain callable (never panics).
    let _ = strategy.try_balance(&vault);
});
