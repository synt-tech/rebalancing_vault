//! Deployment-path integration test: platform.create_vault → registry
//! deploy_strategy → factory create_defindex_vault → registry bind_vault,
//! exercising the real patched-vault + factory WASMs end to end.
#![cfg(test)]
extern crate std;
use super::*;
use soroban_sdk::{
    contractclient, contracttype,
    testutils::{Address as _, Ledger},
    token::{StellarAssetClient, TokenClient},
    vec, Address, BytesN, Env, IntoVal, String, Symbol, Vec,
};

const FACTORY_WASM: &[u8] = include_bytes!(
    "../../../../defindex/apps/contracts/target/wasm32v1-none/release/defindex_factory.wasm"
);
const VAULT_WASM: &[u8] = include_bytes!(
    "../../../../defindex/apps/contracts/target/wasm32v1-none/release/defindex_vault.optimized.wasm"
);
const REGISTRY_WASM: &[u8] =
    include_bytes!("../../target/wasm32v1-none/release/strategy_registry.wasm");
const NULL_ROUTER_WASM: &[u8] =
    include_bytes!("../../target/wasm32v1-none/release/null_router.wasm");
const STRATEGY_WASM: &[u8] =
    include_bytes!("../../target/wasm32v1-none/release/cl_aquarius_strategy.wasm");
const MOCK_WASM: &[u8] = include_bytes!("../../target/wasm32v1-none/release/test_mocks.wasm");

// Mirrors cl_aquarius::Policy / strategy_registry::{Policy, Ceilings,
// StrategyInfo} — XDR-compatible copies for driving WASM-registered
// contracts without cross-crate deps.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Policy {
    pub swap_allowed: Vec<Address>,
    pub oracle_band_bps: u32,
    pub max_oracle_age: u64,
    pub max_slippage_bps: u32,
    pub max_loss_bps_per_window: u32,
    pub loss_window_secs: u64,
    pub cooldown_secs: u64,
    pub range_min_bps: u32,
    pub range_max_bps: u32,
    pub tick_cap_bps: u32,
    pub perf_fee_bps: u32,
    pub platform_split_bps: u32,
    pub min_reward_swap: i128,
    pub max_reward_age: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct Ceilings {
    pub max_perf_fee_bps: u32,
    pub max_slippage_bps: u32,
    pub max_oracle_band_bps: u32,
    pub max_loss_bps_per_window: u32,
    pub min_cooldown_secs: u64,
    pub min_platform_split_bps: u32,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct StrategyInfo {
    pub template_hash: BytesN<32>,
    pub family: Symbol,
    pub deployer: Address,
    pub bound_vault: Option<Address>,
}

// Vault deposit's return tail — mirrors defindex vault models.
#[contracttype]
#[derive(Clone, Debug)]
pub struct StrategyAllocation {
    pub strategy_address: Address,
    pub amount: i128,
    pub paused: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct AssetInvestmentAllocation {
    pub asset: Address,
    pub strategy_allocations: Vec<Option<StrategyAllocation>>,
}

#[contractclient(name = "RegistryTestClient")]
trait RegistryApi {
    fn set_platform(env: Env, platform: Address);
    fn register_template(env: Env, template_hash: BytesN<32>, family: Symbol);
    fn add_venue(env: Env, venue: Address);
    fn set_ceilings(env: Env, ceilings: Ceilings);
    fn treasury(env: Env) -> Address;
    fn strategy_info(env: Env, strategy: Address) -> Option<StrategyInfo>;
}

#[contractclient(name = "VaultTestClient")]
trait VaultApi {
    fn get_emergency_manager(env: Env) -> Address;
    fn get_manager(env: Env) -> Address;
    fn get_fee_receiver(env: Env) -> Address;
    fn get_rebalance_manager(env: Env) -> Address;
    fn deposit(
        env: Env,
        amounts_desired: Vec<i128>,
        amounts_min: Vec<i128>,
        from: Address,
        invest: bool,
    ) -> (Vec<i128>, i128, Option<Vec<AssetInvestmentAllocation>>);
    fn balance(env: Env, from: Address) -> Vec<i128>;
}

#[contractclient(name = "StrategyTestClient")]
trait StrategyApi {
    fn asset(env: Env) -> Address;
    fn balance(env: Env, from: Address) -> i128;
}

fn invoke(e: &Env, addr: &Address, name: &str, args: Vec<Val>) {
    e.invoke_contract::<()>(addr, &Symbol::new(e, name), args);
}

#[test]
fn create_vault_deploys_binds_and_runs_deposit() {
    let e = Env::default();
    e.cost_estimate().budget().reset_unlimited();
    e.mock_all_auths_allowing_non_root_auth();
    e.ledger().set_timestamp(1_700_000_000);

    let admin = Address::generate(&e);
    let treasury = Address::generate(&e);
    let receiver = Address::generate(&e);
    let operator = Address::generate(&e);
    let keeper = Address::generate(&e);

    // Tokens + mocks (pool-shaped __constructor args satisfy every instance).
    let xlm = e
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let usdc = e
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let aqua = e
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let sqrtp: u128 = 125_260_000_000_000_000_000_000_000_000; // √2.5·2^96
    let pool = e.register(MOCK_WASM, (xlm.clone(), usdc.clone(), sqrtp));
    let oracle = e.register(MOCK_WASM, (xlm.clone(), usdc.clone(), 1u128));
    let reward_pool = e.register(MOCK_WASM, (xlm.clone(), aqua.clone(), 1u128));
    for (tok, p) in [
        (usdc.clone(), 100_000_000_000_000i128),
        (xlm.clone(), 40_000_000_000_000i128),
        (aqua.clone(), 20_000_000_000_000i128),
    ] {
        invoke(
            &e,
            &oracle,
            "set_price",
            vec![&e, tok.into_val(&e), p.into_val(&e)],
        );
    }

    // Platform stack: factory → registry → platform → wiring.
    let vault_hash = e.deployer().upload_contract_wasm(VAULT_WASM);
    let template_hash = e.deployer().upload_contract_wasm(STRATEGY_WASM);
    let factory = e.register(
        FACTORY_WASM,
        (admin.clone(), receiver.clone(), 0u32, vault_hash),
    );
    let registry = e.register(
        REGISTRY_WASM,
        (admin.clone(), treasury.clone(), admin.clone()),
    );
    let null_router = e.register(NULL_ROUTER_WASM, ());
    let platform_addr = e.register(
        Platform,
        (
            admin.clone(),
            factory.clone(),
            registry.clone(),
            null_router.clone(),
            treasury.clone(),
        ),
    );
    let platform = PlatformClient::new(&e, &platform_addr);
    let registry_c = RegistryTestClient::new(&e, &registry);

    registry_c.set_platform(&platform_addr);
    registry_c.register_template(&template_hash, &Symbol::new(&e, "cl_aquarius"));
    registry_c.set_ceilings(&Ceilings {
        max_perf_fee_bps: 3000,
        max_slippage_bps: 1000,
        max_oracle_band_bps: 1000,
        max_loss_bps_per_window: 5000,
        min_cooldown_secs: 0,
        min_platform_split_bps: 2000,
    });
    for v in [&pool, &oracle, &reward_pool] {
        registry_c.add_venue(v);
    }

    // init_args per cl_aquarius layout; [2] registry / [3] treasury carry
    // attacker-controlled junk — the registry must pin both (audit A4).
    let junk = Address::generate(&e);
    let policy = Policy {
        swap_allowed: vec![&e, pool.clone(), reward_pool.clone()],
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
    };
    let init_args: Vec<Val> = vec![
        &e,
        pool.clone().into_val(&e),
        oracle.clone().into_val(&e),
        junk.clone().into_val(&e),
        junk.into_val(&e),
        policy.into_val(&e),
        aqua.clone().into_val(&e),
        reward_pool.clone().into_val(&e),
    ];

    let (vault, strategy) = platform.create_vault(
        &operator,
        &template_hash,
        &BytesN::from_array(&e, &[7u8; 32]),
        &usdc,
        &init_args,
        &keeper,
        &String::from_str(&e, "CL Vault"),
        &String::from_str(&e, "dfCL"),
    );

    // Binding + provenance recorded by the registry.
    let info = registry_c.strategy_info(&strategy).unwrap();
    assert_eq!(info.bound_vault, Some(vault.clone()));
    assert_eq!(info.deployer, operator);

    // Vault roles pinned by the platform wrapper.
    let vault_c = VaultTestClient::new(&e, &vault);
    assert_eq!(vault_c.get_emergency_manager(), platform_addr);
    assert_eq!(vault_c.get_manager(), operator);
    assert_eq!(vault_c.get_fee_receiver(), operator);
    assert_eq!(vault_c.get_rebalance_manager(), keeper);

    // Strategy bound to the vault and reporting the right asset.
    let strategy_c = StrategyTestClient::new(&e, &strategy);
    assert_eq!(strategy_c.asset(), usdc);

    // Real deposit through the created vault (idle, no invest).
    let user = Address::generate(&e);
    StellarAssetClient::new(&e, &usdc).mint(&user, &10_000_000_000);
    let (amounts, shares, _) = vault_c.deposit(
        &vec![&e, 10_000_000_000i128],
        &vec![&e, 1i128],
        &user,
        &false,
    );
    assert_eq!(amounts.get(0).unwrap(), 10_000_000_000);
    assert!(shares > 0);
    assert_eq!(TokenClient::new(&e, &usdc).balance(&vault), 10_000_000_000);
}

/// Registry rejects a venue not on the allowlist (audit A9).
#[test]
fn deploy_rejects_unlisted_pool() {
    let e = Env::default();
    e.cost_estimate().budget().reset_unlimited();
    e.mock_all_auths_allowing_non_root_auth();

    let admin = Address::generate(&e);
    let treasury = Address::generate(&e);
    let xlm = e
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let usdc = e
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let aqua = e
        .register_stellar_asset_contract_v2(admin.clone())
        .address();
    let pool = e.register(MOCK_WASM, (xlm.clone(), usdc.clone(), 1u128));
    let oracle = e.register(MOCK_WASM, (xlm.clone(), usdc.clone(), 1u128));
    let reward_pool = e.register(MOCK_WASM, (xlm.clone(), aqua.clone(), 1u128));

    let template_hash = e.deployer().upload_contract_wasm(STRATEGY_WASM);
    let registry = e.register(
        REGISTRY_WASM,
        (admin.clone(), treasury.clone(), admin.clone()),
    );
    let registry_c = RegistryTestClient::new(&e, &registry);
    registry_c.register_template(&template_hash, &Symbol::new(&e, "cl_aquarius"));
    // Deliberately do NOT add_venue(pool).
    registry_c.add_venue(&oracle);
    registry_c.add_venue(&reward_pool);

    let policy = Policy {
        swap_allowed: vec![&e, pool.clone()],
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
    };
    let init_args: Vec<Val> = vec![
        &e,
        pool.clone().into_val(&e),
        oracle.clone().into_val(&e),
        registry.clone().into_val(&e),
        treasury.into_val(&e),
        policy.into_val(&e),
        aqua.into_val(&e),
        reward_pool.into_val(&e),
    ];
    let res = e.try_invoke_contract::<Address, soroban_sdk::Error>(
        &registry,
        &Symbol::new(&e, "deploy_strategy"),
        vec![
            &e,
            Address::generate(&e).into_val(&e),
            template_hash.into_val(&e),
            BytesN::from_array(&e, &[9u8; 32]).into_val(&e),
            usdc.into_val(&e),
            init_args.into_val(&e),
        ],
    );
    assert!(res.is_err());
}
