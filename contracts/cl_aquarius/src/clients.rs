//! Hand-written client traits for the contracts we call.
//! Signatures verified against the deployed mainnet spec of the Aquarius
//! XLM/USDC concentrated pool (CBBMQB…BUCV), 2026-10-25 — the deployed
//! interface is a superset of the local soroban-amm copy.
#![allow(dead_code)]
use crate::storage::Policy;
use soroban_sdk::{contractclient, contracttype, Address, Env, Symbol, Vec, U256};

// ---- Aquarius concentrated pool ----

#[contracttype]
#[derive(Clone, Debug)]
pub struct Slot0 {
    pub sqrt_price_x96: U256,
    pub tick: i32,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PositionData {
    pub fee_growth_inside_0_last_x128: U256,
    pub fee_growth_inside_1_last_x128: U256,
    pub liquidity: u128,
    pub tokens_owed_0: u128,
    pub tokens_owed_1: u128,
}

#[contractclient(name = "ClPoolClient")]
pub trait ClPool {
    fn get_slot0(env: Env) -> Slot0;
    fn get_tokens(env: Env) -> Vec<Address>;
    fn get_position(env: Env, recipient: Address, tick_lower: i32, tick_upper: i32)
        -> PositionData;
    fn get_position_fees(env: Env, owner: Address, tick_lower: i32, tick_upper: i32) -> Vec<u128>;
    fn claim_position_fees(env: Env, owner: Address, tick_lower: i32, tick_upper: i32)
        -> Vec<u128>;
    fn estimate_withdraw_position(
        env: Env,
        owner: Address,
        tick_lower: i32,
        tick_upper: i32,
        amount: u128,
    ) -> Vec<u128>;
    fn withdraw_position(
        env: Env,
        owner: Address,
        tick_lower: i32,
        tick_upper: i32,
        amount: u128,
        min_amounts: Vec<u128>,
    ) -> Vec<u128>;
    fn deposit_position(
        env: Env,
        sender: Address,
        tick_lower: i32,
        tick_upper: i32,
        desired_amounts: Vec<u128>,
        min_liquidity: u128,
    ) -> (Vec<u128>, u128);
    fn swap(
        env: Env,
        user: Address,
        in_idx: u32,
        out_idx: u32,
        in_amount: u128,
        out_min: u128,
    ) -> u128;
    fn swap_strict_receive(
        env: Env,
        user: Address,
        in_idx: u32,
        out_idx: u32,
        out_amount: u128,
        in_max: u128,
    ) -> u128;
    fn estimate_swap(env: Env, in_idx: u32, out_idx: u32, in_amount: u128) -> u128;
    fn get_user_reward(env: Env, user: Address) -> u128;
    fn claim(env: Env, user: Address) -> u128;
    fn set_rewards_state(env: Env, user: Address, state: bool);
    fn get_emergency_mode(env: Env) -> bool;
    fn get_tick_spacing(env: Env) -> i32;
}

// ---- Reflector SEP-40 oracle ----

#[contracttype]
#[derive(Clone, Debug)]
pub enum OracleAsset {
    Stellar(Address),
    Other(Symbol),
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct PriceData {
    pub price: i128,
    pub timestamp: u64,
}

#[contractclient(name = "ReflectorClient")]
pub trait Reflector {
    fn lastprice(env: Env, asset: OracleAsset) -> Option<PriceData>;
    fn decimals(env: Env) -> u32;
}

/// Per-asset oracle route set at init: which feed to query and under
/// which identifier (Reflector feeds disagree — `Other("XLM")` on the
/// external USD feed vs `Stellar(sac)` on the Stellar-DEX feed).
#[contracttype]
#[derive(Clone, Debug)]
pub struct OracleRoute {
    pub asset: Address,
    pub oracle: Address,
    pub asset_id: OracleAsset,
}

// ---- DeFindex vault (role getters only) ----

#[contractclient(name = "VaultClient")]
pub trait Vault {
    fn get_rebalance_manager(env: Env) -> Address;
    fn get_manager(env: Env) -> Address;
    fn get_emergency_manager(env: Env) -> Address;
    fn get_fee_receiver(env: Env) -> Address;
}

// ---- strategy registry (policy ceilings + platform treasury) ----

#[contractclient(name = "RegistryClient")]
pub trait Registry {
    fn validate_policy(env: Env, policy: Policy);
    fn treasury(env: Env) -> Address;
}
