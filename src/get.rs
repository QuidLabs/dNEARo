use crate::*;

use near_sdk::{env, log, Balance, Promise};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::json_types::{WrappedBalance, WrappedTimestamp, U128};

pub trait PledgeForTreeMap: Clone + BorshSerialize + BorshDeserialize {
    fn get_id(&self) -> AccountId;
    fn get_debt_amt(&self, short: bool) -> U128;
    fn get_coll_val(&self, short: bool) -> U128;
    fn get_CR(&self, short: bool, price: u128) -> U128;
}
impl PledgeForTreeMap for Pledge {
    fn get_id(&self) -> AccountId {
        self.id.clone()
    }

    fn get_debt_amt(&self, short: bool) -> U128 {
        if short {
            return self.short.debit.into()
        } else {
            return self.long.debit.into()
        }
    }

    fn get_coll_val(&self, short: bool) -> U128 {
        if short {
            return self.short.credit.into()
        } else {
            return self.long.credit.into()
        }
    }

    fn get_CR(&self, short: bool, price: u128) -> U128 {
        if short {
            return computeCR(price, self.short.credit, self.short.debit, true).into();
        } else {
            return computeCR(price, self.long.credit, self.long.debit, false).into();
        }
    }
}

#[near_bindgen]
impl Contract 
{
    pub fn get_price(&self) -> u128 { 
        return self.price;
    }
    
    pub fn get_vol(&self) -> u128 { 
        return self.vol;
    }

    // // TODO Flux, push-based not pull-based
    // https://github.com/fluxprotocol/fpo-near/blob/main/consumer/src/lib.rs
    // 1req/min rate limited
    pub fn set_price(&mut self, _price: u128) { // TODO remove
        self.price = _price;
    }
    
    pub fn set_vol(&mut self, _vol: u128) { // TODO remove
        self.vol = _vol;
    }    
    
    pub fn get_pool_stats(&self) -> PoolStats {
        PoolStats::new(&self)
    }

    pub fn get_pledge(&self, account: ValidAccountId) -> Option<PledgeView> {
        self.pledges.get(account.as_ref()).map(|a| (&a).into())
    }

    pub fn get_qd_balance(&self, account: ValidAccountId) -> WrappedBalance {
        let balance: WrappedBalance = self.token.ft_balance_of(
            ValidAccountId::try_from(account.clone()).unwrap()
        ).into();   
        return balance;
    }

    pub fn get_pledges(&self, from_index: u64, limit: u64) -> Vec<(AccountId, PledgeView)> {
        let account_ids = self.pledges.keys_as_vector();
        let pledges = self.pledges.values_as_vector();
        (from_index..std::cmp::min(from_index + limit, account_ids.len()))
            .map(|index| {
                let account_id = account_ids.get(index).unwrap();
                let pledge_view = (&pledges.get(index).unwrap()).into();
                (account_id, pledge_view)
            })
            .collect()
    }

    pub fn get_pledge_stats(&self, account: ValidAccountId, short: bool) -> Stats {
        if let Some(pledge) = self.pledges.get(account.as_ref()) {  
            if short {
                return pledge.stats.short.clone();
            } else {
                return pledge.stats.long.clone();
            }
        } else {
            if short {
                return self.stats.short.clone();
            } else {
                return self.stats.long.clone();
            }   
        }
    }

    pub fn get_pledge_tree(&self, from_index: u64, limit: u64) {
        let keys: Vec<AccountId> = self.long_crs.to_vec()
                                .iter()
                                .map(|(u, _)| u.id.clone())
                                .collect();

    }

    pub(crate) fn fetch_pledge(&mut self, id: &AccountId, create: bool) -> Pledge {
        if let Some(mut pledge) = self.pledges.get(&id) 
        {
            let val_near_sp = self.blood.debit
                .checked_div(KILL_CR).expect(ERR_DIV);
                    
            self.stats.val_near_sp = val_near_sp
                .checked_mul(self.get_price()).expect(ERR_MUL);
            
            self.stats.val_total_sp = self.blood.credit
                .checked_add(self.stats.val_near_sp).expect(ERR_ADD);
            
            if self.sp_stress(None, false) > 0.0 // stress the long side of the SolvencyPool
            && self.sp_stress(None, true) > 0.0 { // stress the short side of the SolvencyPool
                // retrieve the Pledge's pending allocation of fees as well as defaulted
                // long/short Pledges' collateral and debt, post redemptions/inversions
                // stressed value of SolvencyPool's short side, exclusing given pledge
                let s_stress_ins_x = self.sp_stress(Some(id.clone()), true);
                let s_delta = self.stats.short.stress_val - s_stress_ins_x;
                let s_pcs: f64 = s_delta / self.stats.short.stress_val; // % contrib. to short solvency
                
                // stressed value of SolvencyPool's long side, exclusing given pledge
                let l_stress_ins_x = self.sp_stress(Some(id.clone()), false);
                let l_delta = self.stats.long.stress_val - l_stress_ins_x;
                let l_pcs: f64 = l_delta / self.stats.long.stress_val; // % contrib. to long solvency
                
                if s_pcs > 0.0 && l_pcs > 0.0 {
                    // Calculate DeadPool shares to absorb by this pledge
                    // TODO attack where net postive DP is drained by repeated 
                    // micro pledge updates...limit updates to occur only once an hour
                    let mut near = (self.dead.long.debit as f64 * l_pcs).round() as Balance;
                    let mut near_debt = (self.dead.short.credit as f64 * s_pcs).round() as Balance;
                    let mut qd_debt = (self.dead.long.credit as f64 * l_pcs).round() as Balance;
                    let mut qd = (self.dead.short.debit as f64 * s_pcs).round() as Balance;
                    
                    if near_debt >= near 
                    { // net loss in terms of NEAR
                        let mut delta = near_debt - near;
                        self.dead.long.debit -= near; // the NEAR gain has been absorbed 
                        if delta > 0 {
                            // absorb as much as we can from the pledge
                            let min = std::cmp::min(pledge.near, delta);
                            self.dead.short.credit -= min; // TODO -= delta
                            
                            pledge.near = pledge.near // decrement user's recorded SP deposit
                                .checked_sub(min).expect(ERR_SUB);
                            self.blood.debit = self.blood.debit // decrement deposit from SP
                                .checked_sub(min).expect(ERR_SUB);
                            delta -= min;
                            if delta > 0 { // TODO calling multiple times will suck all money 
                                // from the cold pool until the DeadPool gets emptied out
                                // limit to once per hour, perhaps? Don't fuck with Batfrog
                                self.gfund.long.credit = self.gfund.long.credit
                                    .checked_sub(delta).expect(ERR_SUB);
                            }
                        }
                    } else 
                    { // net gain in terms of NEAR 
                        self.dead.short.credit -= near_debt;
                        self.dead.long.debit -= near;
                        near -= near_debt;
                        // TODO if pledge has any CR between 100-110, take the smaller one first
                        // add enough NEAR collat to long / remove enough NEAR debt from short
                        // such that the new CR is > 110, repeat again for larger CR side 
                        // remaining NEAR goes to SP deposit...
                        pledge.near = pledge.near
                            .checked_add(near).expect(ERR_ADD);
                        self.blood.debit = self.blood.debit
                            .checked_add(near).expect(ERR_ADD);
                    }
                    if qd_debt >= qd 
                    { // net loss in terms of QD
                        let mut delta = qd_debt - qd;
                        self.dead.short.debit -= qd; // the QD gain has been absorbed
                        if delta > 0 {
                            let min = std::cmp::min(pledge.quid, delta);
                            self.dead.long.credit -= min; // TODO -= delta
                            pledge.quid = pledge.quid
                                .checked_sub(min).expect(ERR_SUB);
                            self.blood.credit = self.blood.credit
                                .checked_sub(min).expect(ERR_SUB);
                            delta -= min;   
                            if delta > 0 {
                                // TODO
                                // use x_margin to absorb into the borrowing position, 
                                // only remainder after x_margin should be absorbed by gfund pool
                                self.gfund.short.credit = self.gfund.short.credit
                                    .checked_sub(delta).expect(ERR_SUB);
                            }
                        }
                    } else 
                    { // net gain in terms of QD
                        self.dead.long.credit -= qd_debt;
                        self.dead.short.debit -= qd;
                        qd -= qd_debt;
                        // TODO if pledge has any CR between 100-110, take the smaller one first
                        // let mut min = std::min(pledge.long.debit, qd);
                        // pledge.long.debit -= min;
                        // qd -= min;
                        // remove enough QD debt such that CR > 110
                        // if short CR between 100-110
                        // add enough QD to collat such that CR > 110 
                        // remaininf QD goes to SP deposit...
                        pledge.quid = pledge.quid
                            .checked_add(qd).expect(ERR_ADD);
                        self.blood.credit = self.blood.credit
                            .checked_add(qd).expect(ERR_ADD);
                    }
                }
            }
            return pledge;
        } 
        else if create {
            let mut prefix = Vec::with_capacity(33);
            prefix.push(b's');
            prefix.extend(env::sha256(id.as_bytes()));
            return Pledge {
                long: Pod::new(0, 0),
                short: Pod::new(0, 0),
                stats: PledgeStats::new(),
                quid: 0, near: 0,
                id: id.clone(),
                target: MIN_CR
            }
        } else {
            env::panic(b"Pledge doesn't exist"); 
        }
    }

}