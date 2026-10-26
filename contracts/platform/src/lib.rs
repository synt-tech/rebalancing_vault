//! platform — operator-facing wrapper for platform vault creation.
//! Per platform_design.md §4.2: atomically deploys a strategy instance via
//! `strategy_registry`, creates the vault through our own stock-factory
//! instance with platform-pinned parameters (vault_fee=0, null_router,
//! upgradable=false, EmergencyManager=platform), then binds strategy↔vault.
//! Also exposes platform-only safety ops (pause/rescue via the
//! EmergencyManager role the wrapper assigns to itself).
#![no_std]
use soroban_sdk::{
    auth::{ContractContext, InvokerContractAuthEntry, SubContractInvocation},
    contract, contracterror, contractimpl, contracttype, vec, Address, BytesN, Env, IntoVal, Map,
    String, Symbol, Val, Vec,
};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum PlatError {
    NotInitialized = 1,
    NotAuthorized = 2,
}

// ---- factory types (mirror defindex common::models) ----

#[contracttype]
#[derive(Clone, Debug)]
pub struct Strategy {
    pub address: Address,
    pub name: String,
    pub paused: bool,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct AssetStrategySet {
    pub address: Address,
    pub strategies: Vec<Strategy>,
}

// RolesDataKey indices in the DeFindex vault.
const ROLE_EMERGENCY: u32 = 0;
const ROLE_FEE_RECEIVER: u32 = 1;
const ROLE_MANAGER: u32 = 2;
const ROLE_REBALANCE: u32 = 3;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Factory,
    Registry,
    NullRouter,
    Treasury,
}

#[contract]
pub struct Platform;

fn require_admin(e: &Env) {
    e.storage()
        .instance()
        .get::<DataKey, Address>(&DataKey::Admin)
        .unwrap()
        .require_auth();
}

fn iget(e: &Env, k: &DataKey) -> Address {
    e.storage().instance().get(k).unwrap()
}

#[contractimpl]
impl Platform {
    pub fn __constructor(
        e: Env,
        admin: Address,
        factory: Address,
        registry: Address,
        null_router: Address,
        treasury: Address,
    ) {
        e.storage().instance().set(&DataKey::Admin, &admin);
        e.storage().instance().set(&DataKey::Factory, &factory);
        e.storage().instance().set(&DataKey::Registry, &registry);
        e.storage()
            .instance()
            .set(&DataKey::NullRouter, &null_router);
        e.storage().instance().set(&DataKey::Treasury, &treasury);
    }

    /// Create a platform vault + bound CL strategy, atomically.
    /// `caller` = operator (becomes vault Manager + VaultFeeReceiver).
    /// init_args order = cl_aquarius __constructor:
    /// [pool, oracle, registry, treasury, policy, reward_token, reward_pool].
    /// Returns (vault, strategy).
    pub fn create_vault(
        e: Env,
        caller: Address,
        template_hash: BytesN<32>,
        salt: BytesN<32>,
        asset: Address,
        init_args: Vec<Val>,
        rebalance_manager: Address,
        vault_name: String,
        vault_symbol: String,
    ) -> (Address, Address) {
        caller.require_auth();
        let registry = iget(&e, &DataKey::Registry);

        // 1. Deploy strategy instance (registry enforces template + ceilings).
        let mut deploy_args: Vec<Val> = Vec::new(&e);
        deploy_args.push_back(caller.clone().into_val(&e));
        deploy_args.push_back(template_hash.into_val(&e));
        deploy_args.push_back(salt.into_val(&e));
        deploy_args.push_back(asset.clone().into_val(&e));
        deploy_args.push_back(init_args.into_val(&e));
        let strategy: Address =
            e.invoke_contract(&registry, &Symbol::new(&e, "deploy_strategy"), deploy_args);

        // 2. Create vault via our stock-factory instance. Platform keeps
        //    EmergencyManager; vault_fee pinned 0; null_router kills the
        //    manager-swap path; upgradable=false (Manager cannot swap WASM).
        let mut roles: Map<u32, Address> = Map::new(&e);
        roles.set(ROLE_EMERGENCY, e.current_contract_address());
        roles.set(ROLE_FEE_RECEIVER, caller.clone());
        roles.set(ROLE_MANAGER, caller.clone());
        roles.set(ROLE_REBALANCE, rebalance_manager);

        let mut name_symbol: Map<String, String> = Map::new(&e);
        name_symbol.set(String::from_str(&e, "name"), vault_name);
        name_symbol.set(String::from_str(&e, "symbol"), vault_symbol);

        let strategy_entry = Strategy {
            address: strategy.clone(),
            name: String::from_str(&e, "cl_aquarius"),
            paused: false,
        };
        let mut strats: Vec<Strategy> = Vec::new(&e);
        strats.push_back(strategy_entry);
        let mut assets: Vec<AssetStrategySet> = Vec::new(&e);
        assets.push_back(AssetStrategySet {
            address: asset,
            strategies: strats,
        });

        let mut factory_args: Vec<Val> = Vec::new(&e);
        factory_args.push_back(roles.into_val(&e));
        factory_args.push_back(0u32.into_val(&e)); // vault_fee = 0
        factory_args.push_back(assets.into_val(&e));
        factory_args.push_back(iget(&e, &DataKey::NullRouter).into_val(&e));
        factory_args.push_back(name_symbol.into_val(&e));
        factory_args.push_back(false.into_val(&e)); // upgradable = false
        let vault: Address = e.invoke_contract(
            &iget(&e, &DataKey::Factory),
            &Symbol::new(&e, "create_defindex_vault"),
            factory_args,
        );

        // 3. Bind strategy ↔ vault (registry records provenance). The registry
        //    calls back into the platform (`platform.require_auth()`), so the
        //    platform must authorize its own contract invocation.
        let mut bind_args: Vec<Val> = Vec::new(&e);
        bind_args.push_back(strategy.clone().into_val(&e));
        bind_args.push_back(vault.clone().into_val(&e));
        e.authorize_as_current_contract(vec![
            &e,
            InvokerContractAuthEntry::Contract(SubContractInvocation {
                context: ContractContext {
                    contract: registry.clone(),
                    fn_name: Symbol::new(&e, "bind_vault"),
                    args: bind_args.clone().into_val(&e),
                },
                sub_invocations: Vec::new(&e),
            }),
        ]);
        e.invoke_contract::<()>(&registry, &Symbol::new(&e, "bind_vault"), bind_args);

        e.events().publish(
            (Symbol::new(&e, "VaultListed"),),
            (vault.clone(), strategy.clone()),
        );
        (vault, strategy)
    }

    /// Platform emergency ops — the wrapper holds EmergencyManager on every
    /// vault it creates, so it can pause/unwind without the Manager.
    pub fn pause_strategy(e: Env, vault: Address, strategy: Address) {
        require_admin(&e);
        let mut args: Vec<Val> = Vec::new(&e);
        args.push_back(strategy.into_val(&e));
        e.invoke_contract::<()>(&vault, &Symbol::new(&e, "pause_strategy"), args);
    }

    pub fn unpause_strategy(e: Env, vault: Address, strategy: Address) {
        require_admin(&e);
        let mut args: Vec<Val> = Vec::new(&e);
        args.push_back(strategy.into_val(&e));
        e.invoke_contract::<()>(&vault, &Symbol::new(&e, "unpause_strategy"), args);
    }

    /// Force-unwind a strategy's full balance into vault idle + pause it.
    pub fn rescue_vault(e: Env, vault: Address, strategy: Address) {
        require_admin(&e);
        let mut args: Vec<Val> = Vec::new(&e);
        args.push_back(strategy.into_val(&e));
        args.push_back(e.current_contract_address().into_val(&e));
        e.invoke_contract::<()>(&vault, &Symbol::new(&e, "rescue"), args);
    }
}

mod test;
