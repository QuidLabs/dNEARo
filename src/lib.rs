/*! QD Fungible Token implementation with JSON serialization.
  - Maximum balance value is limited by U128 (2**128 - 1)...
  - JSON calls must pass U128 as base-10 string. E.g. "100".
  - The contract tracks the change in storage before and after the call. If the storage increases,
    the contract requires the caller of the contract to attach enough deposit to the function call
    to cover the storage cost. This is done to prevent a denial of service attack on the contract.
    If the storage decreases, the contract will issue a refund for the cost of the released storage.
    Unused tokens from the attached deposit are also refunded, so attach more deposit than required.
  - To prevent the deployed contract from abused, it should not have any access keys on its account.
*/ use near_contract_standards::fungible_token::FungibleToken;
use near_contract_standards::fungible_token::receiver::FungibleTokenReceiver;
use near_contract_standards::fungible_token::metadata::{
    FungibleTokenMetadata, FungibleTokenMetadataProvider, FT_METADATA_SPEC
}; 
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::{
    env, log, near_bindgen, AccountId, Balance, PanicOnDefault, 
    PromiseOrValue, Promise, assert_one_yocto 
};
use near_sdk::json_types::{ValidAccountId, U128};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::collections::{
    LazyOption, LookupMap, TreeMap,
    UnorderedMap, UnorderedSet
}; 
use uint::construct_uint;
use std::convert::TryFrom;
use std::convert::TryInto;
near_sdk::setup_alloc!();
construct_uint! {
    /// 256-bit unsigned integer.
    pub struct U256(4);
}
use crate::pledge::*; mod pledge;
use crate::utils::*; mod utils;
use crate::pool::*; mod pool;
use crate::grab::*; mod grab;
use crate::bonk::*; mod bonk;
use crate::get::*; mod get;
use crate::out::*; mod out;

#[serde(crate = "near_sdk::serde")]
#[derive(BorshDeserialize, BorshSerialize, Debug, Serialize)]
pub struct Crank {
    pub done: bool, // currently updating
    pub index: usize, // amount of collateral
    pub last: u64, // timestamp of last time Crank was updated
} impl Crank {
    pub fn new() -> Self {
        Self {
            done: true,
            index: 0,
            last: 0,
        }
    }
}

#[serde(crate = "near_sdk::serde")]
#[derive(BorshSerialize, BorshDeserialize, Debug, Serialize)]
pub struct Data { // Used in weighted median voting for solvency target
    solvency: f64, // capital adequacy needed to back debt
    median: f64, // Median of votes for Solvency Target
    scale: f64, // (scale = target / solvency)
    k: u64, // approx. index of median (+/- 1)
    sum_w_k: Balance, // sum(W[0..k])
    total: Balance,
    // all the distinct votes for given property
    y: Vec<i64>,
    // all the weights associated with the votes
    w: Vec<Balance>
} impl Data {
    pub fn new() -> Self {
        Self { solvency: 1.0, 
            median: -1.0, scale: 1.0,
            k: 0, sum_w_k: 0, total: 0, 
            y: Vec::new(), w: Vec::new()
        }
    }
}

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct Contract { token: FungibleToken, // this contract is NEP141 token
    price: u128,
    vol: u128,
    metadata: LazyOption<FungibleTokenMetadata>,
    //votes: LookupMap<AccountId, f64>, // current solvency target vote...
    data_s: Data, // Data structure related to voting for solvency target
    data_l: Data, // Same, but for the long budget (above is for shorts)
    crank: Crank, // Used in `update` function
    pledges: UnorderedMap<AccountId, Pledge>,
    short_crs: PledgesTreeMap<Pledge, ()>, 
    long_crs: PledgesTreeMap<Pledge, ()>,
    stats: PledgeStats, // Global Risk Vars
    blood: Pod, // Solvency Pool deposits 
    gfund: Pool, // gfundPool, // Guarantee Fund
    live: Pool, // Active borrower assets
    dead: Pool // Defaulted borrower assets
}

// TODO QD SVG decode
const SVG_ICON: &str = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 288 288'%3E%3Cg id='l' data-name='l'%3E%3Cpath d='M187.58,79.81l-30.1,44.69a3.2,3.2,0,0,0,4.75,4.2L191.86,103a1.2,1.2,0,0,1,2,.91v80.46a1.2,1.2,0,0,1-2.12.77L102.18,77.93A15.35,15.35,0,0,0,90.47,72.5H87.34A15.34,15.34,0,0,0,72,87.84V201.16A15.34,15.34,0,0,0,87.34,216.5h0a15.35,15.35,0,0,0,13.08-7.31l30.1-44.69a3.2,3.2,0,0,0-4.75-4.2L96.14,186a1.2,1.2,0,0,1-2-.91V104.61a1.2,1.2,0,0,1,2.12-.77l89.55,107.23a15.35,15.35,0,0,0,11.71,5.43h3.13A15.34,15.34,0,0,0,216,201.16V87.84A15.34,15.34,0,0,0,200.66,72.5h0A15.35,15.35,0,0,0,187.58,79.81Z'/%3E%3C/g%3E%3C/svg%3E";

#[near_bindgen]
impl Contract {
    #[init]
    pub fn new(
        owner_id: ValidAccountId,
    ) -> Self {
        let metadata = FungibleTokenMetadata {
            spec: FT_METADATA_SPEC.to_string(),
            name: "Qu!D".to_string(),
            symbol: "QD".to_string(),
            icon: Some(SVG_ICON.to_string()),
            reference: None,
            reference_hash: None,
            decimals: 24,
        };
        metadata.assert_valid();
        let mut this = Self {
            token: FungibleToken::new(b"q".to_vec()),
            price: ONE, // TODO remove
            vol: 4666066, // TODO remove
            metadata: LazyOption::new(b"m".to_vec(), Some(&metadata)),
            pledges: UnorderedMap::new(b"p".to_vec()),
            short_crs: PledgesTreeMap::new(b"s".to_vec(), Sort::Composite, true),
            long_crs: PledgesTreeMap::new(b"l".to_vec(), Sort::Composite, false),
            data_l: Data::new(),
            data_s: Data::new(),
            crank: Crank::new(),
            stats: PledgeStats::new(), 
            blood: Pod::new(0, 0),
            gfund: Pool::new(), 
            live: Pool::new(),
            dead: Pool::new(),
        };
        this.token.internal_register_account(owner_id.as_ref());
        this
    }

    /*  Weighted Median Algorithm for Solvency Target Voting
	 *  Find value of k in range(1, len(Weights)) such that 
	 *  sum(Weights[0:k]) = sum(Weights[k:len(Weights)+1])
	 *  = sum(Weights) / 2
	 *  If there is no such value of k, there must be a value of k 
	 *  in the same range range(1, len(Weights)) such that 
	 *  sum(Weights[0:k]) > sum(Weights) / 2
	*/
    pub(crate) fn rebalance(&mut self, d: &mut Data, new_total: Balance, 
                            new_stake: Balance, new_vote: i64, 
                            old_stake: Balance, old_vote: i64) {
        d.total = new_total;
        let mut len = d.y.len();
        assert!(len == d.w.len(), "Wrong Weights Length");	
        assert!(new_vote >= 100 && new_vote <= 200, 
        "Allowable SolvencyTarget range is 100-200%");

        let added: bool;
        match d.y.binary_search(&new_vote) {
            Ok(idx) => {
                if new_stake != 0 {
                    d.w[idx] = d.w[idx].saturating_add(new_stake);
                }
                added = false;
            },
            Err(idx) => {
                d.y.insert(idx, new_vote);
                d.w.insert(idx, new_stake);
                added = true;
                len += 1;
            }
        }
        let median = (d.median * 100.0) as i64;
        let mid_stake = d.total.checked_div(2).unwrap_or_else(|| 0);

        if old_vote != -1 && old_stake != 0 { // if not the first time user is voting
            let idx = d.y.binary_search(&old_vote).unwrap_or_else(|x| panic!());
            d.w[idx] = d.w[idx].saturating_sub(old_stake);
            if d.w[idx] == 0 {
                d.y.remove(idx);
                d.w.remove(idx);
                if (idx as u64) >= d.k {
                    d.k -= 1;
                }
                len -= 1;	
            }
        }
        if d.total != 0 && mid_stake != 0 {
            if len == 1 || new_vote <= median {
                d.sum_w_k = d.sum_w_k.saturating_add(new_stake.into());
            }		  
            if old_vote <= median {   
                d.sum_w_k = d.sum_w_k.saturating_sub(old_stake.into());
            }
            if median > new_vote {
                if added && len > 1 {
                    d.k += 1;
                }
                while d.k >= 1 && ((d.sum_w_k.saturating_sub(d.w[d.k as usize])) >= mid_stake) {
                    d.sum_w_k = d.sum_w_k.saturating_sub(d.w[d.k as usize]);
                    d.k -= 1;			
                }
            } else {
                while d.sum_w_k < mid_stake {
                    d.k += 1;
                    d.sum_w_k = d.sum_w_k.saturating_add(d.w[d.k as usize]);
                }
            }
            d.median = (d.y[d.k as usize] as f64) / 100.0; // convert (e.g.) 142 to 1.42
            if d.sum_w_k == mid_stake {
                let intermedian = d.median + (d.y[d.k as usize + 1] as f64) / 100.0;
                d.median = intermedian / 2.0;
            }
        }  else {
            d.sum_w_k = 0;
        }
    } // TODO port from https://github.com/ricktobacco/sputnik-dao-contract/commit/02a66f26bf53f4fa8682a5a461175bdc9afd85f0
        // * update voting weight when user update's their SolvencyDeposit
        // * vote function where user indicates their SolvencyTarget
        /**
            pub fn vote(&mut self, new_vote: i128)  {
                assert_eq!(env::attached_deposit(), 16 * env::storage_byte_cost());
                assert!(new_vote > 0, "Vote cannot be negative");
                let account = env::predecessor_account_id();
                let old_vote: i128;
                if let Some(vote) = self.votes.get(&account) {  
                    old_vote = vote;
                } else {
                    old_vote = -1;
                }
                self.votes.insert(&account, &new_vote);
            }
            pub fn on_stake_change(&mut self, account: AccountId, balances: (Balance, Balance, Balance)) {
                if let Some(old_vote) = self.votes.get(&account) {  
                    self.rebalance(balances.2, balances.1, old_vote, balances.0, old_vote);    
                }
            }
            pub fn on_vote_change(&mut self, old_vote: i128, new_vote: i128, balances: (Balance, Balance)) {
                self.rebalance(balances.1, balances.0, old_vote, balances.0, new_vote);
            }
        */
        
    fn on_account_closed(&mut self, account_id: AccountId, balance: Balance) {
        log!("Closed @{} with {}", account_id, balance);
    }

    fn on_tokens_burned(&mut self, account_id: AccountId, amount: Balance) {
        log!("Account @{} burned {}", account_id, amount);
    }
}

// ======================================================================================

near_contract_standards::impl_fungible_token_core!(Contract, token, on_tokens_burned);
near_contract_standards::impl_fungible_token_storage!(Contract, token, on_account_closed);

#[near_bindgen]
impl FungibleTokenMetadataProvider for Contract {
    fn ft_metadata(&self) -> FungibleTokenMetadata {
        self.metadata.get().unwrap()
    }
}
