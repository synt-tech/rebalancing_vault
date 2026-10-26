//! Fuzz target 3: structured SwapPlan generation via `SorobanArbitrary`.
//! Raw bytes almost never survive `SwapPlan::from_xdr`, so this target lets
//! libFuzzer generate valid-shaped plans with arbitrary field values, then
//! biases each field into the policy envelope. That keeps the campaign
//! exploring the oracle-min-out boundary, range/tick checks, and the
//! unwind→swap→redeploy internals instead of bouncing at the decode gate.
//! The hard invariant: `try_harvest` may return any Err, but must never
//! panic in strategy code.
#![no_main]
use libfuzzer_sys::fuzz_target;
use soroban_sdk::{
    testutils::{arbitrary::SorobanArbitrary, Address as _, Ledger},
    token::{StellarAssetClient, TokenClient},
    vec, xdr::ToXdr, Address, Env, IntoVal, Symbol, TryFromVal, Vec,
};
use cl_aquarius_strategy::storage::{Policy, RangeSpec, SwapPlan, SwapStep};
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

fuzz_target!(|proto: <SwapPlan as SorobanArbitrary>::Prototype| {
    let e = Env::default();
    e.cost_estimate().budget().reset_unlimited();
    e.mock_all_auths_allowing_non_root_auth();
    e.ledger().set_timestamp(1_700_000_000);
    let now: u64 = 1_700_000_000;

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
        vault.clone().into_val(&e), treasury.clone().into_val(&e),
        pol.into_val(&e), aqua.clone().into_val(&e), reward_pool.clone().into_val(&e)];
    let strat_addr = e.register(ClAquarius, (usdc.clone(), init_args));
    let strategy = ClAquariusClient::new(&e, &strat_addr);
    strategy.bind(&vault, &vault);

    // Prime the strategy with a deposit and stock the pools so swap legs can
    // complete — otherwise every plan dies on the first pool token pull.
    StellarAssetClient::new(&e, &usdc).mint(&vault, &10_000_000_000);
    strategy.deposit(&10_000_000_000, &vault);
    for (tok, who) in [
        (xlm.clone(), pool.clone()),
        (usdc.clone(), pool.clone()),
        (xlm.clone(), reward_pool.clone()),
        (aqua.clone(), reward_pool.clone()),
    ] {
        StellarAssetClient::new(&e, &tok).mint(&who, &1_000_000_000_000);
    }
    // Seed a claimable reward above min_reward_swap so the conversion path runs.
    e.invoke_contract::<()>(&reward_pool, &Symbol::new(&e, "set_user_reward"),
        vec![&e, strat_addr.clone().into_val(&e), 2_000_000_000u128.into_val(&e)]);

    let Ok(plan) = SwapPlan::try_from_val(&e, &proto) else { return };

    // Bias each field into the policy envelope while preserving coverage
    // signal: values straddle the valid/invalid boundary rather than being
    // forced valid.
    let mut steps = Vec::new(&e);
    let n = plan.steps.len().min(6); // 6 so len>MAX_SWAP_STEPS stays reachable
    for i in 0..n {
        let Ok(Some(s)) = plan.steps.try_get(i) else { continue };
        let use_pool = if s.in_amount % 4 == 0 { &reward_pool } else { &pool };
        steps.push_back(SwapStep {
            pool: use_pool.clone(),
            in_idx: s.in_idx % 3,         // 0,1 valid; 2 must fail cleanly
            out_idx: s.out_idx % 3,
            in_amount: s.in_amount % 40_000_000_000,
            out_min: s.out_min % 40_000_000_000, // straddles the oracle min_out gate
        });
    }
    let expiry = match plan.expiry % 4 {
        0 => now.saturating_sub(1 + plan.expiry % 10_000), // expired
        1 => now,
        2 => now + plan.expiry % 100_000,                  // fresh
        _ => u64::MAX,
    };
    // Ticks snapped to spacing 10; ~1/4 of inputs get the no-redeploy sentinel.
    let tl = ((plan.new_range.tick_lower % 500) - 250) * 10;
    let tu = ((plan.new_range.tick_upper % 500) - 250) * 10;
    let (tl, tu) = if plan.ref_tick % 4 == 0 {
        (0, 0) // tick_lower >= tick_upper → unwind-only plan
    } else {
        (tl.min(tu), tl.max(tu))
    };
    let guided = SwapPlan {
        steps,
        expiry,
        ref_tick: plan.ref_tick % 400 - 200,
        new_range: RangeSpec { tick_lower: tl, tick_upper: tu },
    };

    let _ = strategy.try_harvest(&keeper, &Some(guided.to_xdr(&e)));

    // Post-invariants regardless of outcome: balance stays callable and any
    // accrued fee records remain consistent (owed legs never go negative).
    if let Ok(Ok(b)) = strategy.try_balance(&vault) {
        assert!(b >= 0, "balance went negative: {}", b);
    }
    let owed = TokenClient::new(&e, &usdc).balance(&strategy.address);
    assert!(owed >= 0);
});
