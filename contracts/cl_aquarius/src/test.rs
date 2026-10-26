//! Strategy unit tests with mock pool/oracle/vault (mocks crate WASM).
#![cfg(test)]
use super::*;
use crate::storage::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{StellarAssetClient, TokenClient},
    vec, xdr::ToXdr, Address, Env, IntoVal, Symbol, Vec,
};

const MOCK_WASM: &[u8] = include_bytes!("../../target/wasm32v1-none/release/test_mocks.wasm");

fn make_policy(_e: &Env, venues: Vec<Address>) -> Policy {
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

struct Fixture {
    e: Env,
    strategy: ClAquariusClient<'static>,
    vault: Address,
    usdc: Address,
    xlm: Address,
    #[allow(dead_code)] aqua: Address,
    #[allow(dead_code)] pool: Address,
    #[allow(dead_code)] reward_pool: Address,
    #[allow(dead_code)] oracle: Address,
    treasury: Address,
    operator: Address,
    keeper: Address,
    emergency: Address,
}

/// Managed pool = XLM/USDC (t0=XLM, t1=USDC, asset=USDC). Spot set so
/// implied ratio = p_usdc/p_xlm = 1e14/4e13 = 2.5e14 → sqrtP = √2.5·2^96.
fn setup() -> Fixture {
    let e = Env::default();
    e.mock_all_auths_allowing_non_root_auth();
    e.ledger().set_timestamp(1_700_000_000);

    let xlm = e.register_stellar_asset_contract_v2(Address::generate(&e));
    let usdc = e.register_stellar_asset_contract_v2(Address::generate(&e));
    let aqua = e.register_stellar_asset_contract_v2(Address::generate(&e));
    let (xlm, usdc, aqua) = (xlm.address(), usdc.address(), aqua.address());

    let sqrtp: u128 = 125_260_000_000_000_000_000_000_000_000; // 1.2526e29 ≈ √2.5·2^96
    let pool = e.register(MOCK_WASM, (xlm.clone(), usdc.clone(), sqrtp));
    let reward_pool = e.register(MOCK_WASM, (xlm.clone(), aqua.clone(), 1u128));
    // same WASM exports __constructor → every instance must satisfy it
    let oracle = e.register(MOCK_WASM, (xlm.clone(), usdc.clone(), 1u128));
    let vault = e.register(MOCK_WASM, (xlm.clone(), usdc.clone(), 1u128));

    let manager = Address::generate(&e);
    let keeper = Address::generate(&e);
    let emergency = Address::generate(&e);
    let operator = Address::generate(&e);
    let treasury = Address::generate(&e);

    // init vault mock (its `init` is a plain fn, not __constructor)
    e.invoke_contract::<()>(&vault, &Symbol::new(&e, "init"), {
        vec![&e, manager.into_val(&e), keeper.into_val(&e), emergency.into_val(&e), operator.into_val(&e)]
    });
    // oracle prices (1e14): usdc=1.0, xlm=0.4, aqua=0.2
    for (tok, p) in [(usdc.clone(), 100_000_000_000_000i128), (xlm.clone(), 40_000_000_000_000i128), (aqua.clone(), 20_000_000_000_000i128)] {
        e.invoke_contract::<()>(&oracle, &Symbol::new(&e, "set_price"), vec![&e, tok.into_val(&e), p.into_val(&e)]);
    }

    let policy = make_policy(&e, vec![&e, pool.clone(), reward_pool.clone()]);
    let init_args: Vec<soroban_sdk::Val> = vec![&e, pool.clone().into_val(&e), oracle.clone().into_val(&e), vault.clone().into_val(&e), treasury.clone().into_val(&e), policy.into_val(&e), aqua.clone().into_val(&e), reward_pool.clone().into_val(&e)];
    let strat_addr = e.register(ClAquarius, (usdc.clone(), init_args));
    let strategy = ClAquariusClient::new(&e, &strat_addr);
    // bind (registry arg reused vault slot — bind expects caller==registry;
    // we passed `vault` as registry above, so bind caller must be `vault`)
    strategy.bind(&vault, &vault);

    Fixture { e, strategy, vault, usdc, xlm, aqua, pool, reward_pool, oracle, treasury, operator, keeper, emergency }
}

fn mint(e: &Env, token: &Address, to: &Address, amt: i128) {
    StellarAssetClient::new(e, token).mint(to, &amt);
}

fn bal(e: &Env, token: &Address, who: &Address) -> i128 {
    TokenClient::new(e, token).balance(who)
}

#[test]
fn deposit_parks_idle_and_balance_reports_it() {
    let f = setup();
    mint(&f.e, &f.usdc, &f.vault, 10_000_000_000);
    let b = f.strategy.deposit(&10_000_000_000, &f.vault);
    assert_eq!(b, 10_000_000_000);
    // idle, no position
    assert_eq!(bal(&f.e, &f.usdc, &f.strategy.address), 10_000_000_000);
    assert_eq!(f.strategy.balance(&f.vault), 10_000_000_000);
    // unbound caller gets 0
    assert_eq!(f.strategy.balance(&Address::generate(&f.e)), 0);
}

#[test]
fn withdraw_idle_delivers_requested() {
    let f = setup();
    mint(&f.e, &f.usdc, &f.vault, 10_000_000_000);
    f.strategy.deposit(&10_000_000_000, &f.vault);
    let got = f.strategy.withdraw(&4_000_000_000, &f.vault, &f.vault);
    assert_eq!(got, 4_000_000_000);
    assert_eq!(bal(&f.e, &f.usdc, &f.strategy.address), 6_000_000_000);
}

#[test]
fn lp_fees_accrue_and_are_claimable() {
    let f = setup();
    mint(&f.e, &f.usdc, &f.vault, 10_000_000_000);
    f.strategy.deposit(&10_000_000_000, &f.vault);
    // create a position via harvest plan
    let plan = SwapPlan { steps: Vec::new(&f.e), expiry: 1_800_000_000, new_range: RangeSpec{tick_lower:-100, tick_upper:100} };
    f.strategy.harvest(&f.keeper, &Some(plan.to_xdr(&f.e)));
    // position created holding 10e9 USDC
    // set claimable fees in token order [t0,t1]: 2e9 XLM + 1e9 USDC
    let tl: i32 = -100; let tu: i32 = 100;
    mint(&f.e, &f.usdc, &f.pool, 1_000_000_000);
    mint(&f.e, &f.xlm, &f.pool, 2_000_000_000);
    f.e.invoke_contract::<()>(&f.pool, &Symbol::new(&f.e, "set_position_fees"),
        vec![&f.e, tl.into_val(&f.e), tu.into_val(&f.e), 2_000_000_000u128.into_val(&f.e), 1_000_000_000u128.into_val(&f.e)]);
    // withdraw → claims + skims 15%: owed .15e9 usdc + .3e9 xlm total,
    // split 50/50 platform/operator
    f.strategy.withdraw(&1_000_000_000, &f.vault, &f.vault);
    assert_eq!(f.strategy.try_claim_fees(&f.treasury, &0u32, &f.usdc).unwrap().unwrap(), 75_000_000);
    assert_eq!(f.strategy.try_claim_fees(&f.operator, &1u32, &f.usdc).unwrap().unwrap(), 75_000_000);
    assert_eq!(f.strategy.try_claim_fees(&f.treasury, &0u32, &f.xlm).unwrap().unwrap(), 150_000_000);
    assert_eq!(f.strategy.try_claim_fees(&f.operator, &1u32, &f.xlm).unwrap().unwrap(), 150_000_000);
}

#[test]
fn harvest_rejects_non_keeper() {
    let f = setup();
    let plan = SwapPlan { steps: Vec::new(&f.e), expiry: 1_800_000_000, new_range: RangeSpec{tick_lower:-100, tick_upper:100} };
    let r = f.strategy.try_harvest(&Address::generate(&f.e), &Some(plan.to_xdr(&f.e)));
    assert!(r.is_err());
}

#[test]
fn oracle_band_revert_blocks_harvest() {
    let f = setup();
    // push spot way outside band: sqrtP for ratio ~100e14
    f.e.invoke_contract::<()>(&f.pool, &Symbol::new(&f.e, "set_spot"),
        vec![&f.e, 7_923_000_000_000_000_000_000_000_000_00u128.into_val(&f.e), 0i32.into_val(&f.e)]);
    let plan = SwapPlan { steps: Vec::new(&f.e), expiry: 1_800_000_000, new_range: RangeSpec{tick_lower:-100, tick_upper:100} };
    assert!(f.strategy.try_harvest(&f.keeper, &Some(plan.to_xdr(&f.e))).is_err());
}

#[test]
fn emergency_exit_unwinds_without_swap() {
    let f = setup();
    mint(&f.e, &f.usdc, &f.vault, 10_000_000_000);
    f.strategy.deposit(&10_000_000_000, &f.vault);
    let plan = SwapPlan { steps: Vec::new(&f.e), expiry: 1_800_000_000, new_range: RangeSpec{tick_lower:-100, tick_upper:100} };
    f.strategy.harvest(&f.keeper, &Some(plan.to_xdr(&f.e)));
    // emergency manager exits — position closed, funds idle in strategy
    f.strategy.emergency_exit(&f.emergency);
    // position gone — balance still ~10e9 as idle USDC
    assert_eq!(f.strategy.balance(&f.vault), 10_000_000_000);
}

// ==================== Integration: real patched vault WASM ====================

const VAULT_WASM: &[u8] = include_bytes!("../../../../defindex/apps/contracts/target/wasm32v1-none/release/defindex_vault.optimized.wasm");

#[soroban_sdk::contracttype]
#[derive(Clone)]
pub struct VaultStrategy {
    pub address: Address,
    pub name: soroban_sdk::String,
    pub paused: bool,
}

#[soroban_sdk::contracttype]
#[derive(Clone)]
pub struct VaultAssetSet {
    pub address: Address,
    pub strategies: Vec<VaultStrategy>,
}

#[soroban_sdk::contracttype]
#[derive(Clone)]
pub enum VaultInstr {
    Unwind(Address, i128),
    Invest(Address, i128),
}

/// Round-trip through the real patched vault: user deposit → keeper
/// Invest → strategy pull → user withdraw (idle=0 → exercises the P2
/// unwind path end-to-end).
#[test]
fn vault_integration_deposit_invest_withdraw() {
    let e = Env::default();
    e.mock_all_auths_allowing_non_root_auth();
    e.ledger().set_timestamp(1_700_000_000);

    let usdc_ct = e.register_stellar_asset_contract_v2(Address::generate(&e));
    let xlm_ct = e.register_stellar_asset_contract_v2(Address::generate(&e));
    let aqua_ct = e.register_stellar_asset_contract_v2(Address::generate(&e));
    let (usdc, xlm, aqua) = (usdc_ct.address(), xlm_ct.address(), aqua_ct.address());

    let manager = Address::generate(&e);
    let keeper = Address::generate(&e);
    let emergency = Address::generate(&e);
    let operator = Address::generate(&e);
    let treasury = Address::generate(&e);
    let registry = Address::generate(&e);
    let user = Address::generate(&e);

    let sqrtp: u128 = 125_260_000_000_000_000_000_000_000_000;
    let pool = e.register(MOCK_WASM, (xlm.clone(), usdc.clone(), sqrtp));
    let reward_pool = e.register(MOCK_WASM, (xlm.clone(), aqua.clone(), 1u128));
    let oracle = e.register(MOCK_WASM, (xlm.clone(), usdc.clone(), 1u128));
    let null_router = e.register(MOCK_WASM, (xlm.clone(), usdc.clone(), 1u128));
    for (tok, p) in [(usdc.clone(), 100_000_000_000_000i128), (xlm.clone(), 40_000_000_000_000i128), (aqua.clone(), 20_000_000_000_000i128)] {
        e.invoke_contract::<()>(&oracle, &Symbol::new(&e, "set_price"), vec![&e, tok.into_val(&e), p.into_val(&e)]);
    }

    // strategy (unbound — vault doesn't exist yet)
    let venues: Vec<Address> = vec![&e, pool.clone(), reward_pool.clone()];
    let policy = make_policy(&e, venues);
    let init_args: Vec<soroban_sdk::Val> = vec![&e,
        pool.clone().into_val(&e), oracle.clone().into_val(&e), registry.clone().into_val(&e),
        treasury.clone().into_val(&e), policy.into_val(&e), aqua.clone().into_val(&e), reward_pool.clone().into_val(&e)];
    let strat_addr = e.register(ClAquarius, (usdc.clone(), init_args));
    let strategy = ClAquariusClient::new(&e, &strat_addr);

    // patched vault: assets=[USDC → [strategy]], roles, fee=0, non-upgradable
    let vault_addr = {
        let mut assets: Vec<VaultAssetSet> = Vec::new(&e);
        let mut strats: Vec<VaultStrategy> = Vec::new(&e);
        strats.push_back(VaultStrategy {
            address: strat_addr.clone(),
            name: soroban_sdk::String::from_str(&e, "cl_aquarius"),
            paused: false,
        });
        assets.push_back(VaultAssetSet { address: usdc.clone(), strategies: strats });
        let mut roles: soroban_sdk::Map<u32, Address> = soroban_sdk::Map::new(&e);
        roles.set(0u32, emergency.clone());
        roles.set(1u32, operator.clone());
        roles.set(2u32, manager.clone());
        roles.set(3u32, keeper.clone());
        let mut name_sym: soroban_sdk::Map<soroban_sdk::String, soroban_sdk::String> = soroban_sdk::Map::new(&e);
        name_sym.set(soroban_sdk::String::from_str(&e, "name"), soroban_sdk::String::from_str(&e, "CLVault"));
        name_sym.set(soroban_sdk::String::from_str(&e, "symbol"), soroban_sdk::String::from_str(&e, "CLV"));
        e.register(VAULT_WASM, (assets, roles, 0u32, treasury.clone(), 2000u32, null_router, name_sym, false))
    };

    // bind strategy↔vault via the registry address the strategy was init'd with
    strategy.bind(&registry, &vault_addr);

    // user deposit 10e9 USDC → shares, leave idle (invest=false)
    StellarAssetClient::new(&e, &usdc).mint(&user, &10_000_000_000);
    let (got_amts, shares, _) = e.invoke_contract::<(Vec<i128>, i128, Option<Vec<soroban_sdk::Val>>)>(
        &vault_addr,
        &Symbol::new(&e, "deposit"),
        vec![&e, Vec::<i128>::from_array(&e, [10_000_000_000i128]).into_val(&e),
             Vec::<i128>::from_array(&e, [0i128]).into_val(&e),
             user.clone().into_val(&e), false.into_val(&e)],
    );
    assert_eq!(got_amts.get(0).unwrap(), 10_000_000_000);
    assert!(shares > 0);

    // keeper invests everything into the strategy
    e.invoke_contract::<()>(&vault_addr, &Symbol::new(&e, "rebalance"),
        vec![&e, keeper.clone().into_val(&e),
             Vec::<VaultInstr>::from_array(&e, [VaultInstr::Invest(strat_addr.clone(), 10_000_000_000i128)]).into_val(&e)]);
    assert_eq!(bal(&e, &usdc, &strat_addr), 10_000_000_000);

    // user withdraws all shares → idle=0 forces the strategy unwind path
    let df_bal: i128 = e.invoke_contract(&vault_addr, &Symbol::new(&e, "balance"),
        vec![&e, user.clone().into_val(&e)]);
    assert!(df_bal > 0 && df_bal <= shares, "df balance mismatch: shares={}", shares);
    let out: Vec<i128> = e.invoke_contract(&vault_addr, &Symbol::new(&e, "withdraw"),
        vec![&e, df_bal.into_val(&e), Vec::<i128>::from_array(&e, [1i128]).into_val(&e), user.clone().into_val(&e)]);
    assert!(out.get(0).unwrap() > 0);
    assert!(out.get(0).unwrap() <= 10_000_000_000);
    assert_eq!(bal(&e, &usdc, &user), out.get(0).unwrap());
}
