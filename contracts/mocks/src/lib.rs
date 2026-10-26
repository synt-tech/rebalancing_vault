//! Test mocks: Aquarius CL pool, Reflector oracle, DeFindex vault roles.
//! Storage-driven, minimal — exercises the strategy's call surface only.
#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, token::TokenClient, Address, Env, Symbol, Vec, U256,
};

/// Must match the deployed Aquarius spec shape exactly (named struct).
#[contracttype]
#[derive(Clone, Debug)]
pub struct Slot0 {
    pub sqrt_price_x96: U256,
    pub tick: i32,
}

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

#[contracttype]
#[derive(Clone)]
pub enum Key {
    Token(u32),
    SqrtX96,
    Tick,
    Pos(Address, i32, i32), // -> (u128 liq, u128 a0, u128 a1)
    PosFees(i32, i32),      // -> Vec<u128> claimable
    UserReward(Address),    // -> u128
    Price(Address),         // -> i128 (1e14 scale)
    Role(u32),              // -> Address
    EmergencyMode,
}

// ---------- Mock CL pool ----------

#[contract]
pub struct MockPool;

#[contractimpl]
impl MockPool {
    pub fn __constructor(e: Env, token0: Address, token1: Address, sqrt_x96_num: u128) {
        e.storage().instance().set(&Key::Token(0), &token0);
        e.storage().instance().set(&Key::Token(1), &token1);
        // sqrt_x96 = sqrt(price1/0)*2^96 — pass numerator scaled: store raw
        e.storage().instance().set(&Key::SqrtX96, &U256::from_u128(&e, sqrt_x96_num));
        e.storage().instance().set(&Key::Tick, &0i32);
    }

    fn t(e: &Env, i: u32) -> Address {
        e.storage().instance().get(&Key::Token(i)).unwrap()
    }

    pub fn get_tokens(e: Env) -> Vec<Address> {
        Vec::from_array(&e, [Self::t(&e, 0), Self::t(&e, 1)])
    }

    pub fn get_slot0(e: Env) -> Slot0 {
        Slot0 {
            sqrt_price_x96: e.storage().instance().get(&Key::SqrtX96).unwrap(),
            tick: e.storage().instance().get(&Key::Tick).unwrap_or(0),
        }
    }

    pub fn set_spot(e: Env, sqrt_x96_num: u128, tick: i32) {
        e.storage().instance().set(&Key::SqrtX96, &U256::from_u128(&e, sqrt_x96_num));
        e.storage().instance().set(&Key::Tick, &tick);
    }

    pub fn get_tick_spacing(_e: Env) -> i32 {
        10
    }

    pub fn get_emergency_mode(_e: Env) -> bool {
        false
    }

    pub fn get_tick(_e: Env, _tick: i32) -> Vec<i128> {
        Vec::new(&_e)
    }

    fn pos(e: &Env, owner: &Address, tl: i32, tu: i32) -> (u128, u128, u128) {
        e.storage()
            .persistent()
            .get(&Key::Pos(owner.clone(), tl, tu))
            .unwrap_or((0u128, 0u128, 0u128))
    }

    fn put_pos(e: &Env, owner: &Address, tl: i32, tu: i32, v: (u128, u128, u128)) {
        e.storage().persistent().set(&Key::Pos(owner.clone(), tl, tu), &v);
    }

    /// (liq, a0, a1) packed into a Vec for a 3-field PositionData-ish return.
    pub fn get_position(e: Env, owner: Address, tl: i32, tu: i32) -> Vec<u128> {
        let (l, a0, a1) = Self::pos(&e, &owner, tl, tu);
        Vec::from_array(&e, [l, a0, a1])
    }

    /// Liquidity is treated 1:1 with "shares"; withdraw pays pro-rata of the
    /// stored deposit amounts (a0,a1 recorded at deposit time).
    pub fn estimate_withdraw_position(e: Env, owner: Address, tl: i32, tu: i32, amount: u128) -> Vec<u128> {
        let (liq, a0, a1) = Self::pos(&e, &owner, tl, tu);
        if liq == 0 {
            return Vec::from_array(&e, [0u128, 0u128]);
        }
        Vec::from_array(&e, [a0 * amount / liq, a1 * amount / liq])
    }

    pub fn withdraw_position(e: Env, owner: Address, tl: i32, tu: i32, amount: u128, min_amounts: Vec<u128>) -> Vec<u128> {
        owner.require_auth();
        let (liq, a0, a1) = Self::pos(&e, &owner, tl, tu);
        if liq == 0 || amount == 0 {
            return Vec::from_array(&e, [0u128, 0u128]);
        }
        let (p0, p1) = (a0 * amount / liq, a1 * amount / liq);
        assert!(p0 >= min_amounts.get(0).unwrap_or(0) && p1 >= min_amounts.get(1).unwrap_or(0), "min_amounts");
        let me = e.current_contract_address();
        if p0 > 0 {
            TokenClient::new(&e, &Self::t(&e, 0)).transfer(&me, &owner, &(p0 as i128));
        }
        if p1 > 0 {
            TokenClient::new(&e, &Self::t(&e, 1)).transfer(&me, &owner, &(p1 as i128));
        }
        Self::put_pos(&e, &owner, tl, tu, (liq - amount, a0 - p0, a1 - p1));
        Vec::from_array(&e, [p0, p1])
    }

    pub fn deposit_position(e: Env, sender: Address, tl: i32, tu: i32, desired: Vec<u128>, _min_liq: u128) -> (Vec<u128>, u128) {
        sender.require_auth();
        let (d0, d1) = (desired.get(0).unwrap_or(0), desired.get(1).unwrap_or(0));
        let me = e.current_contract_address();
        if d0 > 0 {
            TokenClient::new(&e, &Self::t(&e, 0)).transfer(&sender, &me, &(d0 as i128));
        }
        if d1 > 0 {
            TokenClient::new(&e, &Self::t(&e, 1)).transfer(&sender, &me, &(d1 as i128));
        }
        let (liq, a0, a1) = Self::pos(&e, &sender, tl, tu);
        let new_liq = liq + d0 + d1;
        Self::put_pos(&e, &sender, tl, tu, (new_liq, a0 + d0, a1 + d1));
        (Vec::from_array(&e, [d0, d1]), d0 + d1)
    }

    /// Test hook: set claimable position fees.
    pub fn set_position_fees(e: Env, tl: i32, tu: i32, f0: u128, f1: u128) {
        e.storage().persistent().set(&Key::PosFees(tl, tu), &Vec::from_array(&e, [f0, f1]));
    }

    pub fn get_position_fees(e: Env, _owner: Address, tl: i32, tu: i32) -> Vec<u128> {
        e.storage().persistent().get(&Key::PosFees(tl, tu)).unwrap_or(Vec::from_array(&e, [0u128, 0u128]))
    }

    pub fn claim_position_fees(e: Env, owner: Address, tl: i32, tu: i32) -> Vec<u128> {
        owner.require_auth();
        let got: Vec<u128> = e.storage().persistent().get(&Key::PosFees(tl, tu)).unwrap_or(Vec::from_array(&e, [0u128, 0u128]));
        let me = e.current_contract_address();
        if got.get(0).unwrap_or(0) > 0 {
            TokenClient::new(&e, &Self::t(&e, 0)).transfer(&me, &owner, &(got.get(0).unwrap() as i128));
        }
        if got.get(1).unwrap_or(0) > 0 {
            TokenClient::new(&e, &Self::t(&e, 1)).transfer(&me, &owner, &(got.get(1).unwrap() as i128));
        }
        e.storage().persistent().set(&Key::PosFees(tl, tu), &Vec::from_array(&e, [0u128, 0u128]));
        got
    }

    pub fn get_all_position_fees(e: Env, owner: Address) -> Vec<u128> {
        let _ = owner;
        Vec::from_array(&e, [0u128, 0u128])
    }
    pub fn claim_all_position_fees(e: Env, owner: Address) -> Vec<u128> {
        let _ = owner;
        Vec::from_array(&e, [0u128, 0u128])
    }

    /// 1:1 swap — pays exactly `out_min` if pool can cover it (tests choose
    /// out_min ≤ expected). Pulls in_amount.
    pub fn swap(e: Env, user: Address, in_idx: u32, out_idx: u32, in_amount: u128, out_min: u128) -> u128 {
        user.require_auth();
        let me = e.current_contract_address();
        TokenClient::new(&e, &Self::t(&e, in_idx)).transfer(&user, &me, &(in_amount as i128));
        TokenClient::new(&e, &Self::t(&e, out_idx)).transfer(&me, &user, &(out_min as i128));
        out_min
    }

    pub fn swap_strict_receive(e: Env, user: Address, in_idx: u32, out_idx: u32, out_amount: u128, in_max: u128) -> u128 {
        user.require_auth();
        let me = e.current_contract_address();
        TokenClient::new(&e, &Self::t(&e, in_idx)).transfer(&user, &me, &(in_max as i128));
        TokenClient::new(&e, &Self::t(&e, out_idx)).transfer(&me, &user, &(out_amount as i128));
        in_max
    }

    pub fn estimate_swap(_e: Env, _in_idx: u32, _out_idx: u32, in_amount: u128) -> u128 {
        in_amount
    }

    pub fn set_user_reward(e: Env, user: Address, amt: u128) {
        e.storage().persistent().set(&Key::UserReward(user), &amt);
    }
    pub fn get_user_reward(e: Env, user: Address) -> u128 {
        e.storage().persistent().get(&Key::UserReward(user)).unwrap_or(0)
    }
    pub fn claim(e: Env, user: Address) -> u128 {
        user.require_auth();
        let amt: u128 = e.storage().persistent().get(&Key::UserReward(user.clone())).unwrap_or(0);
        e.storage().persistent().set(&Key::UserReward(user), &0u128);
        amt
    }
    pub fn set_rewards_state(_e: Env, _user: Address, _state: bool) {}
    pub fn get_active_liquidity(_e: Env) -> u128 {
        0
    }
}

// ---------- Mock Reflector oracle ----------

#[contract]
pub struct MockOracle;

#[contractimpl]
impl MockOracle {
    pub fn set_price(e: Env, asset: Address, price: i128) {
        e.storage().persistent().set(&Key::Price(asset), &(price, e.ledger().timestamp()));
    }
    pub fn set_price_ts(e: Env, asset: Address, price: i128, ts: u64) {
        e.storage().persistent().set(&Key::Price(asset), &(price, ts));
    }
    /// SEP-40 lastprice — enum arg must match OracleAsset's XDR shape.
    pub fn lastprice(e: Env, asset: OracleAsset) -> Option<PriceData> {
        let a = match asset {
            OracleAsset::Stellar(a) => a,
            OracleAsset::Other(_) => return None,
        };
        let (p, ts): (i128, u64) = e.storage().persistent().get(&Key::Price(a))?;
        Some(PriceData { price: p, timestamp: ts })
    }
    pub fn decimals(_e: Env) -> u32 {
        14
    }
}

// ---------- Mock vault roles ----------

#[contract]
pub struct MockVault;

#[contractimpl]
impl MockVault {
    pub fn init(e: Env, manager: Address, rebalance: Address, emergency: Address, fee_recv: Address) {
        e.storage().instance().set(&Key::Role(0), &emergency);
        e.storage().instance().set(&Key::Role(1), &fee_recv);
        e.storage().instance().set(&Key::Role(2), &manager);
        e.storage().instance().set(&Key::Role(3), &rebalance);
    }
    pub fn get_emergency_manager(e: Env) -> Address {
        e.storage().instance().get(&Key::Role(0)).unwrap()
    }
    pub fn get_fee_receiver(e: Env) -> Address {
        e.storage().instance().get(&Key::Role(1)).unwrap()
    }
    pub fn get_manager(e: Env) -> Address {
        e.storage().instance().get(&Key::Role(2)).unwrap()
    }
    pub fn get_rebalance_manager(e: Env) -> Address {
        e.storage().instance().get(&Key::Role(3)).unwrap()
    }
}
