
use crate::pledge::*; //mod pledge;
use crate::*;

use near_sdk::collections::TreeMap;
use near_sdk::IntoStorageKey;
use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use std::ops::Bound;
use core::f64;

pub const TWO_PI: f64 = 2.0 * PI;
pub const PERIOD: f64 = 1095.0; // = (365*24)/8h of dues 
pub const ONE_HOUR: u64 = 360_000_000_000;
pub const EIGHT_HOURS: u64 = 28_800_000_000_000; // nanosecs
pub const ONE: u128 = 1_000000_000000_000000_000000;
pub const PI: f64 = 3.14159265358979323846264338327950288;
pub const MIN_CR: u128 = 1_100_000_000_000_000_000_000_000;
pub const KILL_CR: u128 = 1_000_000_000_000_000_000_000_000;
pub const DOT_OH_NINE: u128 = 90_909_090_909_090_909_090_909;
pub const FEE: u128 = 9_090_909_090_909_090_909_090; // TODO votable FEE, via SputnikV3
pub const MIN_DEBT: u128 = 90_909_090_909_090_909_090_909_090;

// pub stNEAR: AccountId = "meta-pool.near".parse().unwrap(); // mainnet
// pub stNEAR: AccountId = "meta-v2.pool.testnet".parse().unwrap();
// pub stNEAR: AccountId = "v2.ref-finance.near".parse().unwrap(); // mainnet
// pub stNEAR: AccountId = "v2.ref-finance.testnet".parse().unwrap();

// ======= Error Strings ==================

pub const ERR_ADD: &'static str =
    "Addition overflow";
pub const ERR_DIV: &'static str =
    "Division overflow";
pub const ERR_MUL: &'static str =
    "Multiplication overflow";
pub const ERR_SUB: &'static str =
    "Subtraction underflow";
pub const ERR_BELOW_MIN_CR: &'static str =
    "Cannot do operation that would result in CR below min";
pub const ERR_AMT_TOO_LOW: &'static str = 
    "Amount must be larger than 0";
pub const ERR_MAX_LEVERAGE: &'static str = 
    "Leverage must be between 2-10x";
// TODO
// pub const OldVoteNotFound: &'static str = 
//     "OldVoteNotFound";
// pub const WrongWeightsLength: &'static str = 
//     "WrongWeightsLength";
// pub const MustStakeBeforeVote: &'static str = 
//     "MustStakeBeforeVote";
// pub const ZeroStakeBeforeVote: &'static str = 
//     "ZeroStakeBeforeVote";
// ========================================

#[derive(Debug, BorshDeserialize, BorshSerialize)]
pub enum Sort {
    Composite,
    CollaterlizationRatio,
}

pub fn ratio(multiplier:u128, numerator: u128, denominator: u128) -> u128 { 
    return (
        U256::from(numerator)
            .checked_mul(U256::from(multiplier)).expect("Overflow")
            .checked_div(U256::from(denominator)).expect("Overflow")
    ).as_u128();
}

pub fn computeCR(_price: u128, _collat: u128, _debt: u128, _short: bool) -> u128 {
    if _debt > 0 {
        // assert!(_collat > 0, "never supposed to happen");
        if _collat > 0 {
            if _short {
                let debt = ratio(_price, _debt, ONE);
                return ratio(ONE, _collat, debt);
            } else {
                return ratio(_price, _collat, _debt);
            }
        }
        else {
            return 0;
        }
    } 
    else if _collat > 0 {
        return u128::MAX;
    }
    return 0;
}

// Newton's method of integer square root. 
// pub fn integer_sqrt(value: U256) -> U256 {
//     let mut guess: U256 = (value + U256::one()) >> 1;
//     let mut res = value;
//     while guess < res {
//         res = guess;
//         guess = (value / guess + guess) >> 1;
//     }
//     res
// }

pub fn RationalApproximation(t: f64) -> f64 {
    // Abramowitz and Stegun formula 26.2.23.
    // The absolute value of the error should be less than 4.5 e-4.
    let c: [f64; 3] = [2.515517, 0.802853, 0.010328];
    let d: [f64; 3] = [1.432788, 0.189269, 0.001308];
    t - ((c[2] * t + c[1]) * t + c[0]) / 
        (((d[2] * t + d[1]) * t + d[0]) * t + 1.0)
}

pub fn NormalCDFInverse(p: f64) -> f64 {
    assert!(p > 0.0 && p < 1.0);
    // See article above for explanation of this section.
    if p < 0.5 { // F^-1(p) = -G^-1(p)
        let n: f64 = -2.0 * p.ln();
        return -1.0 * RationalApproximation( n.sqrt() );
    }
    else { // F^-1(p) = G^-1(1-p)
        let l: f64 = 1.0 - p;
        let n: f64 = -2.0 * l.ln();
        return RationalApproximation(n.sqrt());
    }
}

// calculate % loss given short Pledge's portfolio volatility & the statistical assumption of normality
pub fn stress(avg: bool, sqrt_var: f64, short: bool) -> f64 { // max portfolio loss in %
    let mut alpha: f64 = 0.90; // 10% of the worst case scenarios
    if avg {
        alpha = 0.50;  // 50% of the avg case scenarios
    }
    let cdf = NormalCDFInverse(alpha);
    let e1 = -1.0 * (cdf * cdf) / 2.0;
    let mut e2 = ((e1.exp() / TWO_PI.sqrt()) / (1.0 - alpha)) * sqrt_var;
    if short {
        return e2.exp() - 1.0;    
    } else {
        e2 *= -1.0;
        return -1.0 * (e2.exp() - 1.0);
    }
}

// Used for pricing put & call options for borrowers contributing to the ActivePool
pub fn price(payoff: f64, scale: f64, val_crypto: f64, val_quid: f64, ivol: f64, short: bool) -> f64 {
    let max_rate: f64 = 0.42;
    let min_rate: f64 = 0.0042 * scale; // * calibrate
    let sqrt_two: f64 = 2.0_f64.sqrt();
    let div = val_crypto / val_quid;
    let ln = div.ln();
    let d: f64 = (ln + (ivol * ivol / -2.0)/* times calibrate */) / ivol; // * calibrate
    let D = d / sqrt_two;
    let mut rate: f64;
    if short { // erfc is used instead of normal distribution
        rate = (payoff * libm::erfc(-1.0 * D) / 2.0) / val_crypto;
    } else {
        rate = (payoff * libm::erfc(D) / 2.0) / val_quid;
    }
    // rate *= calibrate;
    if rate > max_rate {
        rate = max_rate;
    } else if rate < min_rate {
        rate = min_rate;
    }
    return rate;
}

// ======= TreeMap wrapper for sorting by CR, by 4ire Labs ==================

#[derive(BorshDeserialize, BorshSerialize, Clone)]
pub enum SortKeys<U: PledgeForTreeMap> {
    CRKey { pledge: U, key: (u128, AccountId) },
    CompositeKey { pledge: U, key: (i128, u128, AccountId) },
}

impl<U: PledgeForTreeMap> std::cmp::PartialEq for SortKeys<U> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                SortKeys::CRKey {
                    pledge: _,
                    key: self_key,
                },
                SortKeys::CRKey {
                    pledge: _,
                    key: other_key,
                },
            ) => self_key == other_key,
            (
                SortKeys::CompositeKey {
                    pledge: _,
                    key: self_key,
                },
                SortKeys::CompositeKey {
                    pledge: _,
                    key: other_key,
                },
            ) => self_key == other_key,
            (SortKeys::CRKey { pledge: _, key: _ }, SortKeys::CompositeKey { pledge: _, key: _ }) => {
                env::panic(b"Using different sort")
            }
            (SortKeys::CompositeKey { pledge: _, key: _ }, SortKeys::CRKey { pledge: _, key: _ }) => {
                env::panic(b"Using different sort")
            }
        }
    }
}

impl<U: PledgeForTreeMap> std::cmp::PartialOrd for SortKeys<U> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (
                SortKeys::CRKey {
                    pledge: _,
                    key: self_key,
                },
                SortKeys::CRKey {
                    pledge: _,
                    key: other_key,
                },
            ) => Some(self_key.cmp(other_key)),
            (
                SortKeys::CompositeKey {
                    pledge: _,
                    key: self_key,
                },
                SortKeys::CompositeKey {
                    pledge: _,
                    key: other_key,
                },
            ) => Some(self_key.cmp(other_key)),
            (SortKeys::CRKey { pledge: _, key: _ }, SortKeys::CompositeKey { pledge: _, key: _ }) => {
                env::panic(b"Using different sort")
            }
            (SortKeys::CompositeKey { pledge: _, key: _ }, SortKeys::CRKey { pledge: _, key: _ }) => {
                env::panic(b"Using different sort")
            }
        }
    }
}

impl<U: PledgeForTreeMap> std::cmp::Eq for SortKeys<U> {}
impl<U: PledgeForTreeMap> std::cmp::Ord for SortKeys<U> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (
                SortKeys::CRKey {
                    pledge: _,
                    key: self_key,
                },
                SortKeys::CRKey {
                    pledge: _,
                    key: other_key,
                },
            ) => self_key.cmp(other_key),
            (
                SortKeys::CompositeKey {
                    pledge: _,
                    key: self_key,
                },
                SortKeys::CompositeKey {
                    pledge: _,
                    key: other_key,
                },
            ) => self_key.cmp(other_key),
            (SortKeys::CRKey { pledge: _, key: _ }, SortKeys::CompositeKey { pledge: _, key: _ }) => {
                env::panic(b"Using different sort")
            }
            (SortKeys::CompositeKey { pledge: _, key: _ }, SortKeys::CRKey { pledge: _, key: _ }) => {
                env::panic(b"Using different sort")
            }
        }
    }
}

impl<U: PledgeForTreeMap> SortKeys<U> {
    pub fn new(pledge: &U, id: AccountId, sort: &Sort, short: bool, price: u128) -> Self {
        match sort {
            Sort::Composite => {
                let mut deb = pledge.get_debt_amt(short).0;
                let mut i = 0;
                let magnitude: i128 = loop {
                    if deb / 10 == 0 {
                        break i;
                    }
                    deb /= 10;
                    i += 1;
                };
                SortKeys::CompositeKey {
                    pledge: pledge.clone(),
                    key: (-magnitude, pledge.get_CR(short, price).0, id),
                }
            }
            Sort::CollaterlizationRatio => SortKeys::CRKey {
                pledge: pledge.clone(),
                key: (pledge.get_CR(short, price).0, id),
            },
        }
    }
    pub fn get_pledge(&self) -> U {
        match self {
            SortKeys::CRKey { pledge, key: _ } => pledge.clone(),
            SortKeys::CompositeKey { pledge, key: _ } => pledge.clone(),
        }
    }
}

#[derive(BorshDeserialize, BorshSerialize)]
pub struct PledgesTreeMap<K: PledgeForTreeMap, V: BorshSerialize + BorshDeserialize> {
    value: TreeMap<SortKeys<K>, V>,
    type_of_sort: Sort,
    short: bool
}

impl<V: BorshSerialize + BorshDeserialize, K: PledgeForTreeMap> PledgesTreeMap<K, V> {
    pub fn new<S: IntoStorageKey>(prefix: S, type_of_sort: Sort, short: bool) -> Self {
        let prefix = prefix.into_storage_key();
        PledgesTreeMap {
            type_of_sort,
            value: TreeMap::new([&prefix[..], &[b'v']].concat()),
            short
        }
    }

    pub fn len(&self) -> u64 {
        self.value.len()
    }

    pub fn clear(&mut self) {
        self.value.clear()
    }

    pub fn contains_key(&self, key: &K, price: u128) -> bool {
        self.value
            .contains_key(&SortKeys::new(key, key.get_id(), &self.type_of_sort, self.short, price))
    }

    pub fn get(&self, key: &K, price: u128) -> Option<V> {
        self.value
            .get(&SortKeys::new(key, key.get_id(), &self.type_of_sort, self.short, price))
    }

    pub fn insert(&mut self, key: &K, val: &V, price: u128) -> Option<V> {
        self.value
            .insert(&SortKeys::new(key, key.get_id(), &self.type_of_sort, self.short, price), val)
    }

    pub fn remove(&mut self, key: &K, price: u128) -> Option<V> {
        self.value
            .remove(&SortKeys::new(key, key.get_id(), &self.type_of_sort, self.short, price))
    }

    pub fn min(&self) -> Option<K> {
        self.value.min().map(|sort_key| sort_key.get_pledge())
    }

    pub fn max(&self) -> Option<K> {
        self.value.max().map(|sort_key| sort_key.get_pledge())
    }

    pub fn higher(&self, key: &K, price: u128) -> Option<K> {
        self.value
            .higher(&SortKeys::new(key, key.get_id(), &self.type_of_sort, self.short, price))
            .map(|sort_key| sort_key.get_pledge())
    }

    pub fn lower(&self, key: &K, price: u128) -> Option<K> {
        self.value
            .lower(&SortKeys::new(key, key.get_id(), &self.type_of_sort, self.short, price))
            .map(|sort_key| sort_key.get_pledge())
    }

    pub fn ceil_key(&self, key: &K, price: u128) -> Option<K> {
        self.value
            .ceil_key(&SortKeys::new(key, key.get_id(), &self.type_of_sort, self.short, price))
            .map(|sort_key| sort_key.get_pledge())
    }

    pub fn floor_key(&self, key: &K, price: u128) -> Option<K> {
        self.value
            .floor_key(&SortKeys::new(key, key.get_id(), &self.type_of_sort, self.short, price))
            .map(|sort_key| sort_key.get_pledge())
    }

    pub fn iter<'a>(&'a self) -> impl Iterator<Item = (K, V)> + 'a {
        self.value
            .iter()
            .map(|(sort_key, val)| (sort_key.get_pledge(), val))
    }

    pub fn iter_from<'a>(&'a self, key: K, price: u128) -> impl Iterator<Item = (K, V)> + 'a {
        self.value
            .iter_from(SortKeys::new(&key, key.get_id(), &self.type_of_sort, self.short, price))
            .map(|(sort_key, val)| (sort_key.get_pledge(), val))
    }

    pub fn iter_rev<'a>(&'a self) -> impl Iterator<Item = (K, V)> + 'a {
        self.value
            .iter_rev()
            .map(|(sort_key, val)| (sort_key.get_pledge(), val))
    }

    pub fn iter_rev_from<'a>(&'a self, key: K, price: u128) -> impl Iterator<Item = (K, V)> + 'a {
        self.value
            .iter_rev_from(SortKeys::new(&key, key.get_id(), &self.type_of_sort, self.short, price))
            .map(|(sort_key, val)| (sort_key.get_pledge(), val))
    }

    pub fn range<'a>(&'a self, r: (Bound<K>, Bound<K>), price: u128) -> impl Iterator<Item = (K, V)> + 'a {
        let (lo, hi) = r;
        let lo = match &lo {
            Bound::Included(key) => {
                Bound::Included(SortKeys::new(key, key.get_id(), &self.type_of_sort, self.short, price))
            }
            Bound::Excluded(key) => {
                Bound::Excluded(SortKeys::new(key, key.get_id(), &self.type_of_sort, self.short, price))
            }
            _ => Bound::Unbounded,
        };
        let hi = match &hi {
            Bound::Included(key) => {
                Bound::Included(SortKeys::new(key, key.get_id(), &self.type_of_sort, self.short, price))
            }
            Bound::Excluded(key) => {
                Bound::Excluded(SortKeys::new(key, key.get_id(), &self.type_of_sort, self.short, price))
            }
            _ => Bound::Unbounded,
        };
        self.value
            .range((lo, hi))
            .map(|(sort_key, val)| (sort_key.get_pledge(), val))
    }

    pub fn to_vec(&self) -> Vec<(K, V)> {
        self.iter().collect()
    }

    pub fn get_top(&self, n: usize) -> Vec<(K, V)> {
        self.iter().take(n).collect::<Vec<(K, V)>>()
    }
}

impl<'a, K: PledgeForTreeMap, V: BorshSerialize + BorshDeserialize> IntoIterator
    for &'a PledgesTreeMap<K, V>
{
    type Item = (K, V);
    type IntoIter = PledgesTreeMapIntoIterator<'a, K, V>;

    fn into_iter(self) -> Self::IntoIter {
        PledgesTreeMapIntoIterator {
            map: &self.value,
            current_key: self.value.min(),
        }
    }
}

pub struct PledgesTreeMapIntoIterator<'a, K: PledgeForTreeMap, V: BorshSerialize + BorshDeserialize> {
    map: &'a TreeMap<SortKeys<K>, V>,
    current_key: Option<SortKeys<K>>,
}

impl<'a, K: PledgeForTreeMap, V: BorshSerialize + BorshDeserialize> Iterator
    for PledgesTreeMapIntoIterator<'a, K, V>
{
    type Item = (K, V);
    fn next(&mut self) -> Option<Self::Item> {
        match &self.current_key {
            None => None,
            Some(current_key) => {
                let result = self
                    .map
                    .get(current_key)
                    .map(|val| (current_key.get_pledge(), val));
                self.current_key = self.map.ceil_key(current_key);
                result
            }
        }
    }
}
