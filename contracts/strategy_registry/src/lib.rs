//! strategy_registry — template provenance + deployment for CL strategies.
//! Per platform_design.md §4.2: templates are WASM hashes registered by
//! admin; instances deploy only from registered templates; venue allowlist
//! caps `Policy.swap_allowed`; ceilings cap fee/policy parameters; each
//! instance binds to exactly one vault via `bind_vault` (platform-only).
#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, panic_with_error, Address, BytesN, Env,
    IntoVal, Symbol, Val, Vec,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum RegError {
    NotInitialized = 1,
    NotAuthorized = 2,
    TemplateNotRegistered = 3,
    VenueNotAllowed = 4,
    PolicyExceedsCeiling = 5,
    AlreadyBound = 6,
    UnknownStrategy = 7,
    InvalidInitArgs = 8,
}

/// Mirrors cl_aquarius::Policy — registry must parse it to enforce ceilings.
/// Keep field order in sync with the strategy template.
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

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Treasury,
    Platform,
    Template(BytesN<32>), // hash -> Symbol family
    Venues,
    Ceilings,
    Deployed(Address), // strategy addr -> StrategyInfo
}

#[contract]
pub struct StrategyRegistry;

fn admin(e: &Env) -> Address {
    e.storage().instance().get(&DataKey::Admin).unwrap()
}

fn require_admin(e: &Env) {
    admin(e).require_auth();
}

#[contractimpl]
impl StrategyRegistry {
    pub fn __constructor(e: Env, admin: Address, treasury: Address, platform: Address) {
        e.storage().instance().set(&DataKey::Admin, &admin);
        e.storage().instance().set(&DataKey::Treasury, &treasury);
        e.storage().instance().set(&DataKey::Platform, &platform);
        e.storage().instance().set(&DataKey::Venues, &Vec::<Address>::new(&e));
    }

    // ---- admin ----

    pub fn register_template(e: Env, template_hash: BytesN<32>, family: Symbol) {
        require_admin(&e);
        e.storage().instance().set(&DataKey::Template(template_hash.clone()), &family);
        e.events().publish(
            (Symbol::new(&e, "TemplateRegistered"),),
            (template_hash, family),
        );
    }

    pub fn remove_template(e: Env, template_hash: BytesN<32>) {
        require_admin(&e);
        e.storage().instance().remove(&DataKey::Template(template_hash));
    }

    pub fn add_venue(e: Env, venue: Address) {
        require_admin(&e);
        let mut v: Vec<Address> = e.storage().instance().get(&DataKey::Venues).unwrap_or(Vec::new(&e));
        if !v.contains(&venue) {
            v.push_back(venue.clone());
            e.storage().instance().set(&DataKey::Venues, &v);
            e.events().publish((Symbol::new(&e, "VenueRegistered"),), (venue,));
        }
    }

    pub fn remove_venue(e: Env, venue: Address) {
        require_admin(&e);
        let v: Vec<Address> = e.storage().instance().get(&DataKey::Venues).unwrap_or(Vec::new(&e));
        let mut out = Vec::new(&e);
        for i in 0..v.len() {
            let a = v.get(i).unwrap();
            if a != venue {
                out.push_back(a);
            }
        }
        e.storage().instance().set(&DataKey::Venues, &out);
    }

    pub fn set_ceilings(e: Env, ceilings: Ceilings) {
        require_admin(&e);
        e.storage().instance().set(&DataKey::Ceilings, &ceilings);
    }

    pub fn set_treasury(e: Env, treasury: Address) {
        require_admin(&e);
        e.storage().instance().set(&DataKey::Treasury, &treasury);
    }

    pub fn set_platform(e: Env, platform: Address) {
        require_admin(&e);
        e.storage().instance().set(&DataKey::Platform, &platform);
    }

    // ---- getters ----

    pub fn treasury(e: Env) -> Address {
        e.storage().instance().get(&DataKey::Treasury).unwrap()
    }

    pub fn venues(e: Env) -> Vec<Address> {
        e.storage().instance().get(&DataKey::Venues).unwrap_or(Vec::new(&e))
    }

    pub fn template_family(e: Env, template_hash: BytesN<32>) -> Option<Symbol> {
        e.storage().instance().get(&DataKey::Template(template_hash))
    }

    pub fn strategy_info(e: Env, strategy: Address) -> Option<StrategyInfo> {
        e.storage().instance().get(&DataKey::Deployed(strategy))
    }

    // ---- deployment ----

    /// Deploy a strategy instance from a registered template, enforcing
    /// venue allowlist + policy ceilings. `deployer` is recorded for
    /// provenance (the operator). Returns the instance address.
    /// `init_args` must match the template's __constructor layout
    /// (cl_aquarius: [pool, oracle, registry, treasury, policy, reward_token, reward_pool]).
    /// The registry pins `init_args[2]` (registry) and `init_args[3]`
    /// (treasury) to its own values so the operator cannot redirect fees.
    pub fn deploy_strategy(
        e: Env,
        deployer: Address,
        template_hash: BytesN<32>,
        salt: BytesN<32>,
        asset: Address,
        init_args: Vec<Val>,
    ) -> Address {
        deployer.require_auth();

        let family: Symbol = e
            .storage()
            .instance()
            .get(&DataKey::Template(template_hash.clone()))
            .unwrap_or_else(|| panic_with_error!(&e, RegError::TemplateNotRegistered));

        if init_args.len() < 7 {
            panic_with_error!(&e, RegError::InvalidInitArgs);
        }

        let treasury: Address = e.storage().instance().get(&DataKey::Treasury).unwrap();
        let mut new_init_args: Vec<Val> = Vec::new(&e);
        for i in 0..init_args.len() {
            if i == 2 {
                new_init_args.push_back(e.current_contract_address().into_val(&e));
            } else if i == 3 {
                new_init_args.push_back(treasury.clone().into_val(&e));
            } else {
                new_init_args.push_back(init_args.get(i).unwrap());
            }
        }

        let venues: Vec<Address> = e.storage().instance().get(&DataKey::Venues).unwrap_or(Vec::new(&e));
        for idx in [0u32, 1u32, 6u32] {
            let v = new_init_args.get(idx).unwrap_or_else(|| panic_with_error!(&e, RegError::InvalidInitArgs));
            let a = Address::try_from_val(&e, &v).unwrap_or_else(|_| panic_with_error!(&e, RegError::InvalidInitArgs));
            if !venues.contains(&a) {
                panic_with_error!(&e, RegError::VenueNotAllowed);
            }
        }

        // Policy is init_args[4] for cl_aquarius-family templates.
        let policy_val = new_init_args.get(4).unwrap_or_else(|| panic_with_error!(&e, RegError::InvalidInitArgs));
        let policy = Policy::try_from_val(&e, &policy_val).unwrap_or_else(|_| panic_with_error!(&e, RegError::InvalidInitArgs));
        check_policy(&e, &policy);

        // The strategy constructor is __constructor(asset, init_args).
        let mut deploy_args: Vec<Val> = Vec::new(&e);
        deploy_args.push_back(asset.into_val(&e));
        deploy_args.push_back(new_init_args.into_val(&e));

        let addr = e
            .deployer()
            .with_current_contract(salt)
            .deploy_v2(template_hash.clone(), deploy_args);

        e.storage().instance().set(
            &DataKey::Deployed(addr.clone()),
            &StrategyInfo {
                template_hash: template_hash.clone(),
                family: family.clone(),
                deployer: deployer.clone(),
                bound_vault: None,
            },
        );
        e.events().publish(
            (Symbol::new(&e, "StrategyDeployed"),),
            (addr.clone(), template_hash, deployer),
        );
        addr
    }

    /// Platform-only: bind a deployed strategy to its vault and record it.
    /// Invokes strategy.bind(registry, vault) — the strategy checks that
    /// `caller == registry`, which is satisfied because the registry is the
    /// direct invoker.
    pub fn bind_vault(e: Env, strategy: Address, vault: Address) {
        let platform: Address = e.storage().instance().get(&DataKey::Platform).unwrap();
        platform.require_auth();

        let mut info: StrategyInfo = e
            .storage()
            .instance()
            .get(&DataKey::Deployed(strategy.clone()))
            .unwrap_or_else(|| panic_with_error!(&e, RegError::UnknownStrategy));
        if info.bound_vault.is_some() {
            panic_with_error!(&e, RegError::AlreadyBound);
        }
        e.invoke_contract::<()>(
            &strategy,
            &Symbol::new(&e, "bind"),
            Vec::from_array(&e, [
                e.current_contract_address().into_val(&e),
                vault.clone().into_val(&e),
            ]),
        );
        info.bound_vault = Some(vault.clone());
        e.storage().instance().set(&DataKey::Deployed(strategy.clone()), &info);
        e.events().publish(
            (Symbol::new(&e, "StrategyBound"),),
            (strategy, vault),
        );
    }

    /// Public policy check used by strategies on `set_policy`.
    pub fn validate_policy(e: Env, policy: Policy) {
        check_policy(&e, &policy);
    }
}

fn check_policy(e: &Env, p: &Policy) {
    if let Some(c) = e.storage().instance().get::<DataKey, Ceilings>(&DataKey::Ceilings) {
        if p.perf_fee_bps > c.max_perf_fee_bps
            || p.max_slippage_bps > c.max_slippage_bps
            || p.oracle_band_bps > c.max_oracle_band_bps
            || p.max_loss_bps_per_window > c.max_loss_bps_per_window
            || p.cooldown_secs < c.min_cooldown_secs
            || p.platform_split_bps < c.min_platform_split_bps
        {
            panic_with_error!(e, RegError::PolicyExceedsCeiling);
        }
    }
    let venues: Vec<Address> = e.storage().instance().get(&DataKey::Venues).unwrap_or(Vec::new(e));
    for i in 0..p.swap_allowed.len() {
        if !venues.contains(&p.swap_allowed.get(i).unwrap()) {
            panic_with_error!(e, RegError::VenueNotAllowed);
        }
    }
}

use soroban_sdk::TryFromVal;
