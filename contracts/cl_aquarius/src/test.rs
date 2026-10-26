//! Strategy unit tests with mock pool/oracle/vault (mocks crate WASM).
#![cfg(test)]
extern crate std;
use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token::{StellarAssetClient, TokenClient},
    vec,
    xdr::ToXdr,
    Address, Env, IntoVal, Symbol, Vec,
};
use std::println;

const MOCK_WASM: &[u8] = include_bytes!("../../target/wasm32v1-none/release/test_mocks.wasm");

fn make_policy(_e: &Env, venues: Vec<Address>) -> Policy {
    make_policy_with(_e, venues, 0, 1)
}

fn make_policy_with(
    _e: &Env,
    venues: Vec<Address>,
    cooldown_secs: u64,
    min_reward_swap: i128,
) -> Policy {
    Policy {
        swap_allowed: venues,
        oracle_band_bps: 500,
        max_oracle_age: 600,
        max_slippage_bps: 300,
        max_loss_bps_per_window: 2000,
        loss_window_secs: 86_400,
        cooldown_secs,
        range_min_bps: 50,
        range_max_bps: 50_000,
        tick_cap_bps: 10_000,
        perf_fee_bps: 1500,
        platform_split_bps: 5000,
        min_reward_swap,
        max_reward_age: 3_600,
    }
}

struct Fixture {
    e: Env,
    strategy: ClAquariusClient<'static>,
    vault: Address,
    usdc: Address,
    xlm: Address,
    #[allow(dead_code)]
    aqua: Address,
    #[allow(dead_code)]
    pool: Address,
    #[allow(dead_code)]
    reward_pool: Address,
    #[allow(dead_code)]
    oracle: Address,
    treasury: Address,
    operator: Address,
    keeper: Address,
    emergency: Address,
}

/// Managed pool = XLM/USDC (t0=XLM, t1=USDC, asset=USDC). Spot set so
/// implied ratio = p_usdc/p_xlm = 1e14/4e13 = 2.5e14 → sqrtP = √2.5·2^96.
fn setup() -> Fixture {
    setup_fixture(|e, v| make_policy_with(e, v.clone(), 0, 1))
}

fn setup_with(cooldown_secs: u64, min_reward_swap: i128) -> Fixture {
    setup_fixture(|e, v| make_policy_with(e, v.clone(), cooldown_secs, min_reward_swap))
}

/// Fixture with caller-constructed Policy (loss cap, ceilings tests, etc).
fn setup_fixture(build_policy: impl FnOnce(&Env, &Vec<Address>) -> Policy) -> Fixture {
    let e = Env::default();
    e.cost_estimate().budget().reset_unlimited();
    e.mock_all_auths_allowing_non_root_auth();
    e.ledger().set_timestamp(1_700_000_000);

    let xlm = e.register_stellar_asset_contract_v2(Address::generate(&e));
    let usdc = e.register_stellar_asset_contract_v2(Address::generate(&e));
    let aqua = e.register_stellar_asset_contract_v2(Address::generate(&e));
    let (xlm, usdc, aqua) = (xlm.address(), usdc.address(), aqua.address());

    let sqrtp: u128 = 50_108_289_675_009_587_501_223_378_944; // √0.4·2^96: t0=XLM $0.40, t1=USDC $1.00 → 0.4 t1 per t0
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
        vec![
            &e,
            manager.into_val(&e),
            keeper.into_val(&e),
            emergency.into_val(&e),
            operator.into_val(&e),
        ]
    });
    e.invoke_contract::<()>(&vault, &Symbol::new(&e, "set_treasury"), {
        vec![&e, treasury.into_val(&e)]
    });
    // oracle prices (1e14): usdc=1.0, xlm=0.4, aqua=0.2
    for (tok, p) in [
        (usdc.clone(), 100_000_000_000_000i128),
        (xlm.clone(), 40_000_000_000_000i128),
        (aqua.clone(), 20_000_000_000_000i128),
    ] {
        e.invoke_contract::<()>(
            &oracle,
            &Symbol::new(&e, "set_price"),
            vec![&e, tok.into_val(&e), p.into_val(&e)],
        );
    }

    let venues: Vec<Address> = vec![&e, pool.clone(), reward_pool.clone()];
    let policy = build_policy(&e, &venues);
    let init_args: Vec<soroban_sdk::Val> = vec![
        &e,
        pool.clone().into_val(&e),
        oracle.clone().into_val(&e),
        vault.clone().into_val(&e),
        treasury.clone().into_val(&e),
        policy.into_val(&e),
        aqua.clone().into_val(&e),
        reward_pool.clone().into_val(&e),
    ];
    let strat_addr = e.register(ClAquarius, (usdc.clone(), init_args));
    let strategy = ClAquariusClient::new(&e, &strat_addr);
    // bind (registry arg reused vault slot — bind expects caller==registry;
    // we passed `vault` as registry above, so bind caller must be `vault`)
    strategy.bind(&vault, &vault);

    Fixture {
        e,
        strategy,
        vault,
        usdc,
        xlm,
        aqua,
        pool,
        reward_pool,
        oracle,
        treasury,
        operator,
        keeper,
        emergency,
    }
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
    let plan = SwapPlan {
        steps: Vec::new(&f.e),
        ref_tick: 0,
        expiry: 1_800_000_000,
        new_range: RangeSpec {
            tick_lower: -100,
            tick_upper: 100,
        },
    };
    f.strategy.harvest(&f.keeper, &Some(plan.to_xdr(&f.e)));
    // position created holding 10e9 USDC
    // set claimable fees in token order [t0,t1]: 2e9 XLM + 1e9 USDC
    let tl: i32 = -100;
    let tu: i32 = 100;
    mint(&f.e, &f.usdc, &f.pool, 1_000_000_000);
    mint(&f.e, &f.xlm, &f.pool, 2_000_000_000);
    f.e.invoke_contract::<()>(
        &f.pool,
        &Symbol::new(&f.e, "set_position_fees"),
        vec![
            &f.e,
            tl.into_val(&f.e),
            tu.into_val(&f.e),
            2_000_000_000u128.into_val(&f.e),
            1_000_000_000u128.into_val(&f.e),
        ],
    );
    // withdraw → claims + skims 15%: owed .15e9 usdc + .3e9 xlm total,
    // split 50/50 platform/operator
    f.strategy.withdraw(&1_000_000_000, &f.vault, &f.vault);
    assert_eq!(
        f.strategy
            .try_claim_fees(&f.treasury, &0u32, &f.usdc)
            .unwrap()
            .unwrap(),
        75_000_000
    );
    assert_eq!(
        f.strategy
            .try_claim_fees(&f.operator, &1u32, &f.usdc)
            .unwrap()
            .unwrap(),
        75_000_000
    );
    assert_eq!(
        f.strategy
            .try_claim_fees(&f.treasury, &0u32, &f.xlm)
            .unwrap()
            .unwrap(),
        150_000_000
    );
    assert_eq!(
        f.strategy
            .try_claim_fees(&f.operator, &1u32, &f.xlm)
            .unwrap()
            .unwrap(),
        150_000_000
    );
}

#[test]
fn harvest_rejects_non_keeper() {
    let f = setup();
    let plan = SwapPlan {
        steps: Vec::new(&f.e),
        ref_tick: 0,
        expiry: 1_800_000_000,
        new_range: RangeSpec {
            tick_lower: -100,
            tick_upper: 100,
        },
    };
    let r = f
        .strategy
        .try_harvest(&Address::generate(&f.e), &Some(plan.to_xdr(&f.e)));
    assert!(r.is_err());
}

#[test]
fn oracle_band_revert_blocks_harvest() {
    let f = setup();
    // push spot way outside band: sqrtP for ratio ~100e14
    f.e.invoke_contract::<()>(
        &f.pool,
        &Symbol::new(&f.e, "set_spot"),
        vec![
            &f.e,
            7_923_000_000_000_000_000_000_000_000_00u128.into_val(&f.e),
            0i32.into_val(&f.e),
        ],
    );
    let plan = SwapPlan {
        steps: Vec::new(&f.e),
        ref_tick: 0,
        expiry: 1_800_000_000,
        new_range: RangeSpec {
            tick_lower: -100,
            tick_upper: 100,
        },
    };
    assert!(f
        .strategy
        .try_harvest(&f.keeper, &Some(plan.to_xdr(&f.e)))
        .is_err());
}

#[test]
fn emergency_exit_unwinds_without_swap() {
    let f = setup();
    mint(&f.e, &f.usdc, &f.vault, 10_000_000_000);
    f.strategy.deposit(&10_000_000_000, &f.vault);
    let plan = SwapPlan {
        steps: Vec::new(&f.e),
        ref_tick: 0,
        expiry: 1_800_000_000,
        new_range: RangeSpec {
            tick_lower: -100,
            tick_upper: 100,
        },
    };
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
    e.cost_estimate().budget().reset_unlimited();
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

    let sqrtp: u128 = 50_108_289_675_009_587_501_223_378_944;
    let pool = e.register(MOCK_WASM, (xlm.clone(), usdc.clone(), sqrtp));
    let reward_pool = e.register(MOCK_WASM, (xlm.clone(), aqua.clone(), 1u128));
    let oracle = e.register(MOCK_WASM, (xlm.clone(), usdc.clone(), 1u128));
    let null_router = e.register(MOCK_WASM, (xlm.clone(), usdc.clone(), 1u128));
    for (tok, p) in [
        (usdc.clone(), 100_000_000_000_000i128),
        (xlm.clone(), 40_000_000_000_000i128),
        (aqua.clone(), 20_000_000_000_000i128),
    ] {
        e.invoke_contract::<()>(
            &oracle,
            &Symbol::new(&e, "set_price"),
            vec![&e, tok.into_val(&e), p.into_val(&e)],
        );
    }

    // strategy (unbound — vault doesn't exist yet)
    let venues: Vec<Address> = vec![&e, pool.clone(), reward_pool.clone()];
    let policy = make_policy(&e, venues);
    let init_args: Vec<soroban_sdk::Val> = vec![
        &e,
        pool.clone().into_val(&e),
        oracle.clone().into_val(&e),
        registry.clone().into_val(&e),
        treasury.clone().into_val(&e),
        policy.into_val(&e),
        aqua.clone().into_val(&e),
        reward_pool.clone().into_val(&e),
    ];
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
        assets.push_back(VaultAssetSet {
            address: usdc.clone(),
            strategies: strats,
        });
        let mut roles: soroban_sdk::Map<u32, Address> = soroban_sdk::Map::new(&e);
        roles.set(0u32, emergency.clone());
        roles.set(1u32, operator.clone());
        roles.set(2u32, manager.clone());
        roles.set(3u32, keeper.clone());
        let mut name_sym: soroban_sdk::Map<soroban_sdk::String, soroban_sdk::String> =
            soroban_sdk::Map::new(&e);
        name_sym.set(
            soroban_sdk::String::from_str(&e, "name"),
            soroban_sdk::String::from_str(&e, "CLVault"),
        );
        name_sym.set(
            soroban_sdk::String::from_str(&e, "symbol"),
            soroban_sdk::String::from_str(&e, "CLV"),
        );
        e.register(
            VAULT_WASM,
            (
                assets,
                roles,
                0u32,
                treasury.clone(),
                2000u32,
                null_router,
                name_sym,
                false,
            ),
        )
    };

    // bind strategy↔vault via the registry address the strategy was init'd with
    strategy.bind(&registry, &vault_addr);

    // user deposit 10e9 USDC → shares, leave idle (invest=false)
    StellarAssetClient::new(&e, &usdc).mint(&user, &10_000_000_000);
    let (got_amts, shares, _) = e
        .invoke_contract::<(Vec<i128>, i128, Option<Vec<soroban_sdk::Val>>)>(
            &vault_addr,
            &Symbol::new(&e, "deposit"),
            vec![
                &e,
                Vec::<i128>::from_array(&e, [10_000_000_000i128]).into_val(&e),
                Vec::<i128>::from_array(&e, [0i128]).into_val(&e),
                user.clone().into_val(&e),
                false.into_val(&e),
            ],
        );
    assert_eq!(got_amts.get(0).unwrap(), 10_000_000_000);
    assert!(shares > 0);

    // keeper invests everything into the strategy
    e.invoke_contract::<()>(
        &vault_addr,
        &Symbol::new(&e, "rebalance"),
        vec![
            &e,
            keeper.clone().into_val(&e),
            Vec::<VaultInstr>::from_array(
                &e,
                [VaultInstr::Invest(strat_addr.clone(), 10_000_000_000i128)],
            )
            .into_val(&e),
        ],
    );
    assert_eq!(bal(&e, &usdc, &strat_addr), 10_000_000_000);

    // user withdraws all shares → idle=0 forces the strategy unwind path
    let df_bal: i128 = e.invoke_contract(
        &vault_addr,
        &Symbol::new(&e, "balance"),
        vec![&e, user.clone().into_val(&e)],
    );
    assert!(
        df_bal > 0 && df_bal <= shares,
        "df balance mismatch: shares={}",
        shares
    );
    let out: Vec<i128> = e.invoke_contract(
        &vault_addr,
        &Symbol::new(&e, "withdraw"),
        vec![
            &e,
            df_bal.into_val(&e),
            Vec::<i128>::from_array(&e, [1i128]).into_val(&e),
            user.clone().into_val(&e),
        ],
    );
    assert!(out.get(0).unwrap() > 0);
    assert!(out.get(0).unwrap() <= 10_000_000_000);
    assert_eq!(bal(&e, &usdc, &user), out.get(0).unwrap());
}

#[test]
fn rewards_converted_and_fee_skimmed_at_harvest() {
    let f = setup();
    mint(&f.e, &f.usdc, &f.vault, 10_000_000_000);
    f.strategy.deposit(&10_000_000_000, &f.vault);
    // position first so harvest isn't a no-op
    let plan = SwapPlan {
        steps: Vec::new(&f.e),
        ref_tick: 0,
        expiry: 1_800_000_000,
        new_range: RangeSpec {
            tick_lower: -100,
            tick_upper: 100,
        },
    };
    f.strategy.harvest(&f.keeper, &Some(plan.to_xdr(&f.e)));

    // pending reward 2e9 AQUA; reward pool funded with XLM to pay the swap
    f.e.invoke_contract::<()>(
        &f.pool,
        &Symbol::new(&f.e, "set_user_reward"),
        vec![
            &f.e,
            f.strategy.address.clone().into_val(&f.e),
            2_000_000_000u128.into_val(&f.e),
        ],
    );
    mint(&f.e, &f.aqua, &f.strategy.address, 2_000_000_000); // claim pays from strategy-held AQUA? mock claims w/o paying — provide pool AQUA too
    mint(&f.e, &f.xlm, &f.reward_pool, 100_000_000_000);
    let plan2 = SwapPlan {
        steps: Vec::new(&f.e),
        ref_tick: 0,
        expiry: 1_800_000_000,
        new_range: RangeSpec {
            tick_lower: 0,
            tick_upper: 0,
        },
    }; // keep position
    f.strategy.harvest(&f.keeper, &Some(plan2.to_xdr(&f.e)));
    // claimed 2e9 AQUA → swapped to XLM at oracle price (p_aqua/p_xlm=0.5),
    // mock pays out_min = fair_out×(1-slippage) = 2e9×0.5×0.97 = 970e6 XLM;
    // 15% fee → 145.5e6 XLM total, 50/50 → 72.75e6 each
    let p = f
        .strategy
        .try_claim_fees(&f.treasury, &0u32, &f.xlm)
        .unwrap()
        .unwrap();
    assert!(p > 0, "platform reward fee must accrue");
}

#[test]
fn stale_oracle_blocks_withdraw() {
    let f = setup();
    mint(&f.e, &f.usdc, &f.vault, 10_000_000_000);
    f.strategy.deposit(&10_000_000_000, &f.vault);
    // jump ledger forward past max_oracle_age (600s)
    f.e.ledger().set_timestamp(1_700_000_000 + 700);
    assert!(f
        .strategy
        .try_withdraw(&1_000_000_000, &f.vault, &f.vault)
        .is_err());
}

/// §9 edge: rewards below `min_reward_swap` stay UNCLAIMED in the pool —
/// not stranded as unvalued dust in strategy custody — and cross the gate
/// on a later harvest.
#[test]
fn rewards_below_min_swap_stay_unclaimed() {
    let f = setup_with(0, 10_000);
    mint(&f.e, &f.usdc, &f.vault, 10_000_000_000);
    f.strategy.deposit(&10_000_000_000, &f.vault);
    // pending 100 AQUA ≈ 100×0.2 = 20 asset units < 10_000 → skip claim
    f.e.invoke_contract::<()>(
        &f.pool,
        &Symbol::new(&f.e, "set_user_reward"),
        vec![
            &f.e,
            f.strategy.address.clone().into_val(&f.e),
            100u128.into_val(&f.e),
        ],
    );
    let plan = SwapPlan {
        steps: Vec::new(&f.e),
        ref_tick: 0,
        expiry: 1_800_000_000,
        new_range: RangeSpec {
            tick_lower: -100,
            tick_upper: 100,
        },
    };
    f.strategy.harvest(&f.keeper, &Some(plan.to_xdr(&f.e)));
    let pending: u128 = f.e.invoke_contract(
        &f.pool,
        &Symbol::new(&f.e, "get_user_reward"),
        vec![&f.e, f.strategy.address.clone().into_val(&f.e)],
    );
    assert_eq!(
        pending, 100,
        "below-gate reward must stay unclaimed in the pool"
    );
    assert_eq!(
        bal(&f.e, &f.aqua, &f.strategy.address),
        0,
        "no dust in strategy custody"
    );
}

/// §9 edge: unclaimed rewards never count toward `balance()` — an exiting
/// holder can't be paid for rewards they forfeit.
#[test]
fn unclaimed_rewards_excluded_from_balance() {
    let f = setup();
    mint(&f.e, &f.usdc, &f.vault, 10_000_000_000);
    f.strategy.deposit(&10_000_000_000, &f.vault);
    let before = f.strategy.balance(&f.vault);
    f.e.invoke_contract::<()>(
        &f.pool,
        &Symbol::new(&f.e, "set_user_reward"),
        vec![
            &f.e,
            f.strategy.address.clone().into_val(&f.e),
            5_000_000_000u128.into_val(&f.e),
        ],
    );
    assert_eq!(
        f.strategy.balance(&f.vault),
        before,
        "unclaimed rewards must not move balance()"
    );
}

/// §9 edge: a withdrawal must not claim rewards — rewards forfeit to the
/// remaining holders and are only claimed at harvest.
#[test]
fn withdraw_does_not_claim_rewards() {
    let f = setup();
    mint(&f.e, &f.usdc, &f.vault, 10_000_000_000);
    f.strategy.deposit(&10_000_000_000, &f.vault);
    f.e.invoke_contract::<()>(
        &f.pool,
        &Symbol::new(&f.e, "set_user_reward"),
        vec![
            &f.e,
            f.strategy.address.clone().into_val(&f.e),
            5_000_000_000u128.into_val(&f.e),
        ],
    );
    f.strategy.withdraw(&4_000_000_000, &f.vault, &f.vault);
    let pending: u128 = f.e.invoke_contract(
        &f.pool,
        &Symbol::new(&f.e, "get_user_reward"),
        vec![&f.e, f.strategy.address.clone().into_val(&f.e)],
    );
    assert_eq!(pending, 5_000_000_000, "withdraw must never claim rewards");
}

#[test]
fn expired_plan_is_rejected() {
    let f = setup();
    let plan = SwapPlan {
        steps: Vec::new(&f.e),
        ref_tick: 0,
        expiry: 1_600_000_000,
        new_range: RangeSpec {
            tick_lower: -100,
            tick_upper: 100,
        },
    }; // expiry < ledger 1_700_000_000
    assert!(f
        .strategy
        .try_harvest(&f.keeper, &Some(plan.to_xdr(&f.e)))
        .is_err());
}

#[test]
fn cooldown_blocks_second_harvest() {
    let f = setup_with(120, 1);
    mint(&f.e, &f.usdc, &f.vault, 10_000_000_000);
    f.strategy.deposit(&10_000_000_000, &f.vault);
    let plan = SwapPlan {
        steps: Vec::new(&f.e),
        ref_tick: 0,
        expiry: 1_800_000_000,
        new_range: RangeSpec {
            tick_lower: -100,
            tick_upper: 100,
        },
    };
    f.strategy
        .harvest(&f.keeper, &Some(plan.clone().to_xdr(&f.e)));
    assert!(
        f.strategy
            .try_harvest(&f.keeper, &Some(plan.clone().to_xdr(&f.e)))
            .is_err(),
        "second harvest inside the cooldown must fail"
    );
    // past the window it runs again (prices still fresh: +121 < 600s)
    f.e.ledger().set_timestamp(1_700_000_000 + 121);
    f.strategy.harvest(&f.keeper, &Some(plan.to_xdr(&f.e)));
}

/// §9 edge: platform + operator legs sum exactly to the performance fee —
/// no rounding drift, 50/50 split splits evenly.
#[test]
fn fee_legs_sum_exactly() {
    let f = setup();
    mint(&f.e, &f.usdc, &f.vault, 10_000_000_000);
    f.strategy.deposit(&10_000_000_000, &f.vault);
    let plan = SwapPlan {
        steps: Vec::new(&f.e),
        ref_tick: 0,
        expiry: 1_800_000_000,
        new_range: RangeSpec {
            tick_lower: -100,
            tick_upper: 100,
        },
    };
    f.strategy.harvest(&f.keeper, &Some(plan.to_xdr(&f.e))); // opens the position
                                                             // accrue claimable fees [t0,t1] = 2e9 XLM + 1e9 USDC; fund the pool
    mint(&f.e, &f.usdc, &f.pool, 1_000_000_000);
    mint(&f.e, &f.xlm, &f.pool, 2_000_000_000);
    f.e.invoke_contract::<()>(
        &f.pool,
        &Symbol::new(&f.e, "set_position_fees"),
        vec![
            &f.e,
            (-100i32).into_val(&f.e),
            100i32.into_val(&f.e),
            2_000_000_000u128.into_val(&f.e),
            1_000_000_000u128.into_val(&f.e),
        ],
    );
    let plan2 = SwapPlan {
        steps: Vec::new(&f.e),
        ref_tick: 0,
        expiry: 1_800_000_000,
        new_range: RangeSpec {
            tick_lower: 0,
            tick_upper: 0,
        },
    }; // no redeploy
    f.strategy.harvest(&f.keeper, &Some(plan2.to_xdr(&f.e)));
    let p0 = f.strategy.claim_fees(&f.treasury, &0u32, &f.xlm);
    let o0 = f.strategy.claim_fees(&f.treasury, &1u32, &f.xlm);
    let p1 = f.strategy.claim_fees(&f.treasury, &0u32, &f.usdc);
    let o1 = f.strategy.claim_fees(&f.treasury, &1u32, &f.usdc);
    assert_eq!(
        p0 + o0,
        2_000_000_000 * 1500 / 10_000,
        "fee legs must sum to perf fee on t0"
    );
    assert_eq!(
        p1 + o1,
        1_000_000_000 * 1500 / 10_000,
        "fee legs must sum to perf fee on t1"
    );
    assert_eq!(p0, o0, "50% platform split splits evenly");
}

#[test]
fn claim_fees_permissionless_pays_resolved_recipient() {
    let f = setup();
    // anyone may trigger; payout destination is resolved on-chain
    // (role 0 → treasury), never chosen by the caller.
    let rando = Address::generate(&f.e);
    assert_eq!(
        f.strategy
            .try_claim_fees(&rando, &0u32, &f.usdc)
            .unwrap()
            .unwrap(),
        0
    );
}

#[test]
fn gen_swapplan_xdr() {
    let e = Env::default();
    e.cost_estimate().budget().reset_unlimited();
    let plan = SwapPlan {
        steps: Vec::new(&e),
        ref_tick: 0,
        expiry: 1_900_000_000,
        new_range: RangeSpec {
            tick_lower: -100,
            tick_upper: 100,
        },
    };
    let b = plan.to_xdr(&e);
    let mut hex = std::string::String::new();
    for i in 0..b.len() {
        use core::fmt::Write;
        write!(hex, "{:02x}", b.get(i).unwrap()).unwrap();
    }
    println!("PLAN_XDR={}", hex);
}

#[test]
fn decode_plan_from_hex() {
    let e = Env::default();
    e.cost_estimate().budget().reset_unlimited();
    let hex = "0000001100000001000000040000000f0000000665787069727900000000000500000000713fb3000000000f000000096e65775f72616e67650000000000001100000001000000020000000f0000000a7469636b5f6c6f776572000000000004ffffff9c0000000f0000000a7469636b5f7570706572000000000004000000640000000f000000087265665f7469636b00000004000000000000000f000000057374657073000000000000100000000100000000";
    let mut v = std::vec::Vec::<u8>::new();
    for i in (0..hex.len()).step_by(2) {
        v.push(u8::from_str_radix(&hex[i..i + 2], 16).unwrap());
    }
    let b = soroban_sdk::Bytes::from_slice(&e, &v);
    let r = SwapPlan::from_xdr(&e, &b);
    assert!(r.is_ok());
}

// ==================== Loss budget ====================

/// Swap step that realizes a ~2% loss: `out_min` is set safely above the
/// validation floor (fair × (1 − max_slippage)=97%), and the mock pays
/// `out_min` — so the value destroyed is deterministic.
fn lossy_step(pool: &Address, in_idx: u32, in_amt: u128, fair_out: u128) -> SwapStep {
    SwapStep {
        pool: pool.clone(),
        in_idx,
        out_idx: 1 - in_idx,
        in_amount: in_amt,
        out_min: fair_out * 98 / 100,
    }
}

/// USDC→XLM fair_out = in × $usdc/$xlm = in × 2.5; XLM→USDC = in × 0.4.
fn plan_of(e: &Env, step: SwapStep, expiry: u64) -> SwapPlan {
    SwapPlan {
        steps: vec![e, step],
        ref_tick: 0,
        expiry,
        new_range: RangeSpec {
            tick_lower: 0,
            tick_upper: 0,
        },
    }
}

/// Cumulative loss across harvests is tracked per window; exceeding
/// `max_loss_bps_per_window` reverts — atomically, so the lossy swap in
/// the rejected harvest is rolled back too.
#[test]
fn loss_budget_accumulates_then_reverts_at_cap() {
    let f = setup_fixture(|e, v| {
        let mut p = make_policy_with(e, v.clone(), 0, 1);
        p.max_loss_bps_per_window = 300; // one ~200bps loss fits; two don't
        p
    });
    mint(&f.e, &f.usdc, &f.vault, 10_000_000_000);
    f.strategy.deposit(&10_000_000_000, &f.vault);
    mint(&f.e, &f.xlm, &f.pool, 200_000_000_000); // swap payout liquidity
    mint(&f.e, &f.usdc, &f.pool, 100_000_000_000);

    // Harvest 1: swap all USDC→XLM, mock pays 98% of fair → ~200bps loss. OK.
    let plan1 = plan_of(
        &f.e,
        lossy_step(&f.pool, 1, 10_000_000_000, 25_000_000_000),
        2_000_000_000,
    );
    f.strategy.harvest(&f.keeper, &Some(plan1.to_xdr(&f.e)));

    // Harvest 2 (same window): swap the XLM back at 98% → cumulative ~400bps
    // > 300 cap → must revert; atomicity means the swap is rolled back too.
    let xlm_bal = bal(&f.e, &f.xlm, &f.strategy.address);
    let plan2 = plan_of(
        &f.e,
        lossy_step(&f.pool, 0, xlm_bal as u128, xlm_bal as u128 * 2 / 5),
        2_000_000_000,
    );
    let res = f.strategy.try_harvest(&f.keeper, &Some(plan2.to_xdr(&f.e)));
    assert!(res.is_err(), "cumulative loss over cap must revert");
    assert_eq!(
        bal(&f.e, &f.xlm, &f.strategy.address),
        xlm_bal,
        "rejected harvest must roll back its swaps"
    );
}

/// A fresh loss window starts after `loss_window_secs`: a second loss
/// that would have breached the old window's budget passes in the new one.
#[test]
fn loss_budget_resets_after_window() {
    let f = setup_fixture(|e, v| {
        let mut p = make_policy_with(e, v.clone(), 0, 1);
        p.max_loss_bps_per_window = 300;
        p
    });
    mint(&f.e, &f.usdc, &f.vault, 10_000_000_000);
    f.strategy.deposit(&10_000_000_000, &f.vault);
    mint(&f.e, &f.xlm, &f.pool, 200_000_000_000); // swap payout liquidity
    mint(&f.e, &f.usdc, &f.pool, 100_000_000_000);

    // harvest 1: USDC→XLM lossy. used ≈ 200 ≤ 300.
    let plan1 = plan_of(
        &f.e,
        lossy_step(&f.pool, 1, 10_000_000_000, 25_000_000_000),
        2_000_000_000,
    );
    f.strategy.harvest(&f.keeper, &Some(plan1.to_xdr(&f.e)));

    // harvest 2 same window → cumulative ≈ 400 > 300 → revert.
    let xlm_bal = bal(&f.e, &f.xlm, &f.strategy.address);
    let plan2 = plan_of(
        &f.e,
        lossy_step(&f.pool, 0, xlm_bal as u128, xlm_bal as u128 * 2 / 5),
        2_000_000_000,
    );
    assert!(f
        .strategy
        .try_harvest(&f.keeper, &Some(plan2.to_xdr(&f.e)))
        .is_err());

    // Past the 86400s window the tracker resets — the same loss passes.
    // (h2 was reverted, so the strategy still holds the XLM from h1.
    // The oracle prices must be re-seeded: they age out at 600s too.)
    f.e.ledger().set_timestamp(1_700_000_000 + 86_401);
    for (tok, p) in [
        (f.usdc.clone(), 100_000_000_000_000i128),
        (f.xlm.clone(), 40_000_000_000_000i128),
    ] {
        f.e.invoke_contract::<()>(
            &f.oracle,
            &Symbol::new(&f.e, "set_price"),
            vec![&f.e, tok.into_val(&f.e), p.into_val(&f.e)],
        );
    }
    let xlm_bal2 = bal(&f.e, &f.xlm, &f.strategy.address);
    let plan3 = plan_of(
        &f.e,
        lossy_step(&f.pool, 0, xlm_bal2 as u128, xlm_bal2 as u128 * 2 / 5),
        2_000_000_000,
    );
    f.strategy.harvest(&f.keeper, &Some(plan3.to_xdr(&f.e)));
}

/// A swap leg that validates but fails at execution (overdraws the
/// strategy balance) must revert the WHOLE harvest — the earlier leg,
/// the position unwind, and the LastHarvest write all roll back.
#[test]
fn mid_harvest_failure_reverts_atomically() {
    let f = setup();
    mint(&f.e, &f.usdc, &f.vault, 10_000_000_000);
    f.strategy.deposit(&10_000_000_000, &f.vault);
    mint(&f.e, &f.xlm, &f.pool, 200_000_000_000); // swap payout liquidity
    mint(&f.e, &f.usdc, &f.pool, 100_000_000_000);
    let deploy = SwapPlan {
        steps: Vec::new(&f.e),
        ref_tick: 0,
        expiry: 1_800_000_000,
        new_range: RangeSpec {
            tick_lower: -100,
            tick_upper: 100,
        },
    };
    f.strategy.harvest(&f.keeper, &Some(deploy.to_xdr(&f.e)));
    let bal_before = f.strategy.balance(&f.vault);

    // Step 1 is valid+small; step 2 validates (out_min ≥ floor) but pulls
    // 1e18 USDC from a strategy holding ~0 idle → SAC transfer traps.
    let huge_in: u128 = 1_000_000_000_000_000_000;
    let bad = SwapPlan {
        steps: vec![
            &f.e,
            SwapStep {
                pool: f.pool.clone(),
                in_idx: 1,
                out_idx: 0,
                in_amount: 1_000_000,
                out_min: 2_450_000,
            },
            SwapStep {
                pool: f.pool.clone(),
                in_idx: 1,
                out_idx: 0,
                in_amount: huge_in,
                out_min: 2_425_000_000_000_000_000,
            },
        ],
        ref_tick: 0,
        expiry: 1_800_000_000,
        new_range: RangeSpec {
            tick_lower: -100,
            tick_upper: 100,
        },
    };
    assert!(f
        .strategy
        .try_harvest(&f.keeper, &Some(bad.to_xdr(&f.e)))
        .is_err());
    assert_eq!(
        f.strategy.balance(&f.vault),
        bal_before,
        "failed harvest must leave balance untouched"
    );
    // Position survived: an emergency unwind still pays out the funds.
    f.strategy.emergency_exit(&f.emergency);
    assert_eq!(f.strategy.balance(&f.vault), bal_before);
}

// ==================== Reward oracle resilience ====================

/// If the reward token's feed is missing/stale, harvest must skip the
/// claim instead of reverting — a dead AQUA feed cannot brick rebalance.
#[test]
fn stale_reward_oracle_skips_claim() {
    let f = setup();
    mint(&f.e, &f.usdc, &f.vault, 10_000_000_000);
    f.strategy.deposit(&10_000_000_000, &f.vault);
    f.e.invoke_contract::<()>(
        &f.pool,
        &Symbol::new(&f.e, "set_user_reward"),
        vec![
            &f.e,
            f.strategy.address.clone().into_val(&f.e),
            5_000_000_000u128.into_val(&f.e),
        ],
    );
    // AQUA price timestamped at 0 → age >> max_oracle_age → stale.
    f.e.invoke_contract::<()>(
        &f.oracle,
        &Symbol::new(&f.e, "set_price_ts"),
        vec![
            &f.e,
            f.aqua.clone().into_val(&f.e),
            20_000_000_000_000i128.into_val(&f.e),
            0u64.into_val(&f.e),
        ],
    );
    let plan = SwapPlan {
        steps: Vec::new(&f.e),
        ref_tick: 0,
        expiry: 1_800_000_000,
        new_range: RangeSpec {
            tick_lower: -100,
            tick_upper: 100,
        },
    };
    f.strategy.harvest(&f.keeper, &Some(plan.to_xdr(&f.e)));
    let pending: u128 = f.e.invoke_contract(
        &f.pool,
        &Symbol::new(&f.e, "get_user_reward"),
        vec![&f.e, f.strategy.address.clone().into_val(&f.e)],
    );
    assert_eq!(
        pending, 5_000_000_000,
        "stale reward feed must leave rewards unclaimed, not brick harvest"
    );
}

// ==================== In-kind rescue ====================

/// When the swap route is unavailable, `rescue` pushes both raw token
/// legs to the vault — the path `withdraw` cannot take.
#[test]
fn rescue_delivers_both_legs_in_kind() {
    let f = setup();
    mint(&f.e, &f.usdc, &f.vault, 10_000_000_000);
    f.strategy.deposit(&10_000_000_000, &f.vault);
    let deploy = SwapPlan {
        steps: Vec::new(&f.e),
        ref_tick: 0,
        expiry: 1_800_000_000,
        new_range: RangeSpec {
            tick_lower: -100,
            tick_upper: 100,
        },
    };
    f.strategy.harvest(&f.keeper, &Some(deploy.to_xdr(&f.e)));
    // unwind to custody, then rescue both legs to the vault
    f.strategy.emergency_exit(&f.emergency);
    // strategy now holds idle USDC (all it ever had)
    let got = f.strategy.rescue(&f.emergency);
    assert_eq!(got.get(0).unwrap(), 0, "no XLM leg held");
    assert_eq!(got.get(1).unwrap(), 10_000_000_000);
    assert_eq!(bal(&f.e, &f.usdc, &f.vault), 10_000_000_000);
    assert_eq!(bal(&f.e, &f.usdc, &f.strategy.address), 0);
}

#[test]
fn rescue_rejects_unauthorized() {
    let f = setup();
    mint(&f.e, &f.usdc, &f.vault, 1_000_000_000);
    f.strategy.deposit(&1_000_000_000, &f.vault);
    // keeper is not EmergencyManager/Manager
    assert!(f.strategy.try_rescue(&f.keeper).is_err());
}

// ==================== set_policy ceilings (real registry WASM) ====================

const REGISTRY_WASM: &[u8] =
    include_bytes!("../../target/wasm32v1-none/release/strategy_registry.wasm");

#[soroban_sdk::contracttype]
#[derive(Clone)]
pub struct Ceilings {
    pub max_perf_fee_bps: u32,
    pub max_slippage_bps: u32,
    pub max_oracle_band_bps: u32,
    pub max_loss_bps_per_window: u32,
    pub min_cooldown_secs: u64,
    pub min_platform_split_bps: u32,
}

/// Fixture with the REAL registry wasm wired as the strategy's registry —
/// `set_policy` calls `registry.validate_policy`, so ceiling enforcement
/// only exists when the callee is the real contract.
fn setup_with_real_registry() -> (Fixture, Address) {
    let f = setup();
    let e = &f.e;
    let admin = Address::generate(e);
    let platform = Address::generate(e);
    let registry = e.register(REGISTRY_WASM, (admin, f.treasury.clone(), platform));
    e.invoke_contract::<()>(
        &registry,
        &Symbol::new(e, "set_ceilings"),
        vec![
            e,
            Ceilings {
                max_perf_fee_bps: 3000,
                max_slippage_bps: 1000,
                max_oracle_band_bps: 1000,
                max_loss_bps_per_window: 5000,
                min_cooldown_secs: 0,
                min_platform_split_bps: 2000,
            }
            .into_val(e),
        ],
    );
    for v in [&f.pool, &f.reward_pool, &f.oracle] {
        e.invoke_contract::<()>(
            &registry,
            &Symbol::new(e, "add_venue"),
            vec![e, v.clone().into_val(e)],
        );
    }
    // re-point the strategy's registry slot at the real contract and re-bind
    let policy = make_policy(&e, vec![&e, f.pool.clone(), f.reward_pool.clone()]);
    let init_args: Vec<soroban_sdk::Val> = vec![
        e,
        f.pool.clone().into_val(e),
        f.oracle.clone().into_val(e),
        registry.clone().into_val(e),
        f.treasury.clone().into_val(e),
        policy.into_val(e),
        f.aqua.clone().into_val(e),
        f.reward_pool.clone().into_val(e),
    ];
    let strat_addr = e.register(ClAquarius, (f.usdc.clone(), init_args));
    let strategy = ClAquariusClient::new(&e, &strat_addr);
    strategy.bind(&registry, &f.vault);
    (Fixture { strategy, ..f }, registry)
}

#[test]
fn set_policy_within_ceiling_applies() {
    let (f, _registry) = setup_with_real_registry();
    let mut p = make_policy(&f.e, vec![&f.e, f.pool.clone(), f.reward_pool.clone()]);
    p.perf_fee_bps = 2500; // ≤ 3000 ceiling
    p.max_slippage_bps = 500; // ≤ 1000
    p.platform_split_bps = 4000; // ≥ 2000
    f.strategy.set_policy(&f.keeper, &p);
}

#[test]
fn set_policy_over_ceiling_reverts() {
    let (f, _registry) = setup_with_real_registry();
    let mut p = make_policy(&f.e, vec![&f.e, f.pool.clone(), f.reward_pool.clone()]);
    p.perf_fee_bps = 4000; // > 3000 ceiling
    assert!(f.strategy.try_set_policy(&f.keeper, &p).is_err());
}

#[test]
fn set_policy_unlisted_venue_reverts() {
    let (f, _registry) = setup_with_real_registry();
    let rogue = Address::generate(&f.e);
    let mut p = make_policy(&f.e, vec![&f.e, f.pool.clone(), f.reward_pool.clone()]);
    p.swap_allowed.push_back(rogue);
    assert!(f.strategy.try_set_policy(&f.keeper, &p).is_err());
}

#[test]
fn set_policy_rejects_non_keeper() {
    let (f, _registry) = setup_with_real_registry();
    let p = make_policy(&f.e, vec![&f.e, f.pool.clone(), f.reward_pool.clone()]);
    let rando = Address::generate(&f.e);
    assert!(f.strategy.try_set_policy(&rando, &p).is_err());
}
