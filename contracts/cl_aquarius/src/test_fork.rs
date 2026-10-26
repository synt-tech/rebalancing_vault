//! Fork test against real mainnet state: the live Aquarius XLM/USDC
//! concentrated pool, the Reflector DEX feed, and the real XLM/USDC/AQUA
//! stellar-asset contracts, loaded from `testing/fork/mainnet_pool.json`
//! (`stellar snapshot create --archive-url <core-live> --address …`).
//!
//! This test exists because unit fixtures can be self-consistent while
//! disagreeing with reality — the oracle-band ratio inversion survived
//! every mock-based test. Only real pool/oracle state settles:
//!   - the deployed pool's actual ABI (slot0/tokens/spacing/positions),
//!   - Reflector's `Asset::Stellar` addressing and price freshness units,
//!   - our spot↔oracle band convention against live values.
//!
//! Run: `cargo test -p cl_aquarius_strategy -- --ignored`
#![cfg(test)]
extern crate std;
use super::*;
use soroban_sdk::{
    testutils::Address as _, token::TokenClient, vec, xdr::ToXdr, Address, Env, IntoVal, Symbol,
    Vec,
};
use std::println;

const MOCK_WASM: &[u8] = include_bytes!("../../target/wasm32v1-none/release/test_mocks.wasm");
const SNAPSHOT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../testing/fork/mainnet_pool_sdk.json"
);

// Real mainnet contracts (see testing/fork/README of the snapshot).
// Verified live: no single public Reflector feed covers all three assets —
// XLM/USDC exist only on the external CEX+DEX USD feed (identifier
// `Other(symbol)`), AQUA only on the Stellar-DEX feed (`Stellar(sac)`).
const POOL: &str = "CBBMQBNHB2FYVZYV7VNHOJHUMTFJLR4PUMRVQYNW6RHIKZO2NQMIBUCV"; // XLM/USDC concentrated
const REFLECTOR_DEX: &str = "CALI2BYU2JE6WVRUFYTS6MSBNEHGJ35P4AVCZYF3B6QOE3QKOB2PLE6M"; // AQUA feed
const REFLECTOR_EXT: &str = "CAFJZQWSED6YAWZU3GWRTOCNPPCGBN32L7QV43XX5LZLFTK6JLN34DLN"; // XLM/USDC feed
const REWARD_POOL: &str = "CAVB532FDYEWBXNSCSKACTZI3BV3QBNFLH2UQRR5HIKFKLFVQW5YGZVY"; // XLM/AQUA concentrated
const XLM: &str = "CAS3J7GYLGXMF6TDJBBYYSE3HQ6BBSMLNUQ34T6TZMYMW2EVH34XOWMA"; // native SAC (pool t0)
const USDC: &str = "CCW67TSZV3SSS2HXMBQ5JFGCKJNXKZM7UQUWUZPUTHXSTZLEO7SJMI75";
const AQUA: &str = "CAUIKL3IYGMERDRUN6YSCLWVAKIFG5Q4YJHUKM4S4NJZQIA3BAS6OJPK";
// Largest USDC Balance entry in the snapshot (~55M) — funds the vault via
// transfer because `mint` needs the issuer's classic account (not snapshotted).
const USDC_WHALE: &str = "CAHEWHOPPDBQYFMAOLDOXXGUX2BCR7EXP4CWYCRY3NEAJB35YPZMMJFF";

fn addr(e: &Env, s: &str) -> Address {
    Address::from_str(e, s)
}

fn policy(_e: &Env, venues: Vec<Address>) -> Policy {
    Policy {
        swap_allowed: venues,
        oracle_band_bps: 1000, // 10% — real spot vs feed can sit wide
        max_oracle_age: 3_600,
        max_slippage_bps: 500,
        max_loss_bps_per_window: 2000,
        loss_window_secs: 86_400,
        cooldown_secs: 0,
        range_min_bps: 50,
        range_max_bps: 50_000,
        tick_cap_bps: 20_000,
        perf_fee_bps: 1500,
        platform_split_bps: 5000,
        min_reward_swap: 1,
        max_reward_age: 86_400,
    }
}

/// Deploy the strategy against the REAL pool/oracle/reward-pool with a
/// mock vault for role getters, then exercise the full flow.
#[test]
#[ignore = "requires the mainnet ledger snapshot; run with --ignored"]
fn fork_real_pool_full_flow() {
    let e = Env::from_ledger_snapshot_file(SNAPSHOT);
    e.mock_all_auths_allowing_non_root_auth();
    // Real pool + Reflector WASMs exceed the default test budget.
    e.cost_estimate().budget().reset_unlimited();

    let pool_addr = addr(&e, POOL);
    let oracle_ext = addr(&e, REFLECTOR_EXT);
    let oracle_dex = addr(&e, REFLECTOR_DEX);
    let reward_addr = addr(&e, REWARD_POOL);
    let usdc = addr(&e, USDC);
    let xlm = addr(&e, XLM);
    let aqua = addr(&e, AQUA);

    // --- 1. Real ABI reads ---
    let pool = ClPoolClient::new(&e, &pool_addr);
    let slot0 = pool.get_slot0();
    let tokens = pool.get_tokens();
    let spacing = pool.get_tick_spacing();
    let tick = slot0.tick;
    let t0 = tokens.get(0).unwrap();
    let t1 = tokens.get(1).unwrap();
    println!(
        "POOL slot0.tick={} spacing={} t0={:?} t1={:?}",
        tick, spacing, t0, t1
    );

    let ext = ReflectorClient::new(&e, &oracle_ext);
    let dex = ReflectorClient::new(&e, &oracle_dex);
    for (label, a) in [("usdc", "USDC"), ("xlm", "XLM"), ("aqua", "AQUA")] {
        let pd = ext.try_lastprice(&OracleAsset::Other(Symbol::new(&e, a)));
        println!("REFLECTOR_EXT Other({}) -> {:?}", label, pd);
    }
    println!(
        "REFLECTOR_DEX Stellar(AQUA) -> {:?}",
        dex.try_lastprice(&OracleAsset::Stellar(aqua.clone()))
    );

    // --- 2. Deploy strategy bound to a mock vault ---
    let manager = Address::generate(&e);
    let keeper = Address::generate(&e);
    let emergency = Address::generate(&e);
    let operator = Address::generate(&e);
    let treasury = Address::generate(&e);
    let vault = e.register(MOCK_WASM, (xlm.clone(), usdc.clone(), 1u128));
    e.invoke_contract::<()>(
        &vault,
        &Symbol::new(&e, "init"),
        vec![
            &e,
            manager.into_val(&e),
            keeper.clone().into_val(&e),
            emergency.into_val(&e),
            operator.into_val(&e),
        ],
    );
    e.invoke_contract::<()>(
        &vault,
        &Symbol::new(&e, "set_treasury"),
        vec![&e, treasury.clone().into_val(&e)],
    );

    let pol = policy(&e, vec![&e, pool_addr.clone(), reward_addr.clone()]);
    // Per-asset oracle routes: XLM/USDC via the external USD feed under
    // `Other` symbols, AQUA via the DEX feed under its SAC address.
    let mut routes: Vec<OracleRoute> = Vec::new(&e);
    for (a, sym) in [(xlm.clone(), "XLM"), (usdc.clone(), "USDC")] {
        routes.push_back(OracleRoute {
            asset: a,
            oracle: oracle_ext.clone(),
            asset_id: OracleAsset::Other(Symbol::new(&e, sym)),
        });
    }
    routes.push_back(OracleRoute {
        asset: aqua.clone(),
        oracle: oracle_dex.clone(),
        asset_id: OracleAsset::Stellar(aqua.clone()),
    });
    let init_args: Vec<soroban_sdk::Val> = vec![
        &e,
        pool_addr.clone().into_val(&e),
        oracle_ext.clone().into_val(&e),
        vault.clone().into_val(&e),
        treasury.clone().into_val(&e),
        pol.into_val(&e),
        aqua.clone().into_val(&e),
        reward_addr.clone().into_val(&e),
        routes.into_val(&e),
    ];
    // Register the strategy as its compiled WASM when the artifact exists so
    // resource metering covers real VM execution; fall back to native
    // registration when it hasn't been built.
    let wasm_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../target/wasm32v1-none/release/cl_aquarius_strategy.wasm"
    );
    let strat_addr = match std::fs::read(wasm_path) {
        Ok(wasm) => {
            println!("strategy registration: wasm ({} bytes)", wasm.len());
            e.register_at(
                &<Address as soroban_sdk::testutils::Address>::generate(&e),
                wasm.as_slice(),
                (usdc.clone(), init_args),
            )
        }
        Err(_) => {
            println!("strategy registration: native (wasm artifact missing)");
            e.register(ClAquarius, (usdc.clone(), init_args))
        }
    };
    let strategy = ClAquariusClient::new(&e, &strat_addr);
    strategy.bind(&vault, &vault);

    // --- 3. Fund the vault with real USDC via a whale transfer ---
    // (SAC `mint` reads the issuer's classic account entry, which the
    // snapshot does not contain; transfer only needs the sender's
    // Balance entry + receive side, both present.)
    let amt: i128 = 10_000_000; // 1 USDC
    TokenClient::new(&e, &usdc).transfer(&addr(&e, USDC_WHALE), &vault, &amt);
    let b = strategy.deposit(&amt, &vault);
    println!("deposit -> {}", b);
    println!("deposit resources: {:?}", e.cost_estimate().resources());
    assert!(b > 0);

    // --- 4. Harvest with a real-shaped plan: unwind-only (no redeploy) ---
    // proves decode+validate+band+reward+lp-fee paths against real WASM.
    let plan = SwapPlan {
        steps: Vec::new(&e),
        ref_tick: tick,
        expiry: e.ledger().timestamp() + 3600,
        new_range: RangeSpec {
            tick_lower: 0,
            tick_upper: 0,
        },
    };
    let r = strategy.try_harvest(&keeper, &Some(plan.to_xdr(&e)));
    println!("harvest(unwind-only) -> {:?}", r);
    println!("unwind resources: {:?}", e.cost_estimate().resources());
    assert!(r.is_ok(), "unwind-only harvest must succeed on real state");

    // --- 5. Redeploy: single-sided range fully ABOVE the current tick
    // (a range above spot is token0-only; whichever token that is, the
    // pool pulls only what the geometry needs — strategy holds only USDC
    // so we let the real pool's amount math decide).
    let usdc_is_t0 = t0 == usdc;
    // Uniswap-v3 semantics: price below the range => position holds only
    // token0; above => only token1. USDC is t1 here, so the USDC-only side
    // is the band BELOW spot.
    let width = (500 / spacing) * spacing;
    let (tl, tu) = if usdc_is_t0 {
        let lo = ((tick / spacing) + 1) * spacing;
        (lo, lo + width)
    } else {
        let hi = ((tick / spacing) - 1) * spacing;
        (hi - width, hi)
    };
    println!("redeploy range [{}, {}] usdc_is_t0={}", tl, tu, usdc_is_t0);
    let plan2 = SwapPlan {
        steps: Vec::new(&e),
        ref_tick: tick,
        expiry: e.ledger().timestamp() + 3600,
        new_range: RangeSpec {
            tick_lower: tl,
            tick_upper: tu,
        },
    };
    // Not try_: on failure we want the host's event trace to locate the abort.
    strategy.harvest(&keeper, &Some(plan2.to_xdr(&e)));
    println!("harvest(redeploy) -> ok");
    println!("redeploy resources: {:?}", e.cost_estimate().resources());

    // --- 6. Balance + partial withdraw through the open real position ---
    let bal = strategy.balance(&vault);
    println!("balance -> {}", bal);
    assert!(bal > 0);
    let w = strategy.try_withdraw(&(amt / 2), &vault, &vault);
    println!("withdraw {} -> {:?}", amt / 2, w);
    println!("withdraw resources: {:?}", e.cost_estimate().resources());
    assert!(w.is_ok());
}
