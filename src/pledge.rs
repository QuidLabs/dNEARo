
use crate::*;
use libm;

use near_sdk::{env, log, Balance, Promise};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::json_types::{WrappedBalance, WrappedTimestamp, U128};

#[serde(crate = "near_sdk::serde")]
#[derive(BorshDeserialize, BorshSerialize, Clone, Serialize)]
pub struct Stats {
    pub val_near: Balance, // $ value of crypto assets
    pub stress_val: f64, //  $ value of the Solvency Pool in stress 
    pub avg_val: f64, // $ value of the Solvency Pool in average stress 
    pub stress_loss: f64, // % loss that Solvency pool would suffer in a stress event
    pub avg_loss: f64, // % loss that Solvency pool would suffer in an average stress event
    pub premiums: f64, // $ amount of premiums borrower would pay in a year to insure their collateral
    pub rate: f64, // annualized rate borrowers pay in periodic premiums to insure their collateral
}
impl Stats {
    pub fn new() -> Self {
        Self {
            val_near: 0,
            stress_val: 0.0,
            avg_val: 0.0,
            stress_loss: 0.0,
            avg_loss: 0.0,
            premiums: 0.0,
            rate: 0.0,
        }
    }
    pub fn clone(&self) -> Self {
        Self {
            val_near: self.val_near.clone(),
            stress_val: self.stress_val.clone(),
            avg_val: self.avg_val.clone(),
            stress_loss: self.stress_loss.clone(),
            avg_loss: self.avg_loss.clone(),
            premiums: self.premiums.clone(),
            rate: self.rate.clone(),
        }
    }
}

#[serde(crate = "near_sdk::serde")]
#[derive(BorshDeserialize, BorshSerialize, Clone, Serialize)]
pub struct PledgeStats {
    pub long: Stats,
    pub short: Stats,
    pub val_near_sp: Balance, // $ value of the NEAR solvency deposit
    pub val_total_sp: Balance, // total $ value of val_near plus $QD solvency deposit
}
impl PledgeStats {
    pub fn new() -> Self {
        Self {
            long: Stats::new(),
            short: Stats::new(),
            val_near_sp: 0,
            val_total_sp: 0
        }
    }
    pub fn clone(&self) -> Self {
        Self {
            long: self.long.clone(), 
            short: self.short.clone(),
            val_near_sp: self.val_near_sp.clone(),
            val_total_sp: self.val_total_sp.clone()
        }
    }
}

#[serde(crate = "near_sdk::serde")]
#[derive(BorshDeserialize, BorshSerialize, Clone, Serialize)]
pub struct Pledge { // each User is a Pledge, whether or not borrowing
    // borrowing users will have non-zero values in `long` and `short`
    pub long: Pod, // debt in $QD, collateral in NEAR
    pub short: Pod, // debt in NEAR, collateral in $QD
    // pub assets: Pool, // pledge.assets.long.debit
    // instead of pledge.long.debit
    pub stats: PledgeStats, // risk management metrics
    pub near: Balance, // SolvencyPool deposit of NEAR
    pub quid: Balance, // SolvencyPool deposit of $QD
    pub id: AccountId,
    pub target: u128
}
/*
 * Every great magic trick consists of three parts or acts. 
 * The first part is called "The Pledge". The magician shows 
 * you something ordinary: a deck of Troves, a cake or a pie.
 * He shows you this object, perhaps asks you to inspect it,
 * to see if it is indeed real, unaltered, normal...it's not.
*/
impl Pledge {
    pub fn clone(&self) -> Self {
        Self {
            long: self.long.clone(),
            short: self.short.clone(),
            stats: self.stats.clone(),
            near: self.near,
            quid: self.quid,
            id: self.id.clone(),
            target: self.target
        }
    }
}

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

#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct PledgeView {
    pub debit: WrappedBalance,
    pub s_debit: WrappedBalance,
    pub credit: WrappedBalance,
    pub s_credit: WrappedBalance,
    pub quid_sp: WrappedBalance,
    pub near_sp: WrappedBalance
}
 
impl From<&Pledge> for PledgeView {
    fn from(p: &Pledge) -> Self {
        Self {
            debit: p.long.debit.into(),
            s_debit: p.short.debit.into(),
            credit: p.long.credit.into(),
            s_credit: p.short.credit.into(),
            near_sp: p.near.into(),
            quid_sp: p.quid.into(),
        }
    }
}

#[near_bindgen]
impl Contract {
    pub(crate) fn save_pledge(&mut self, id: &AccountId,  pledge: &mut Pledge, long_touched: bool, short_touched: bool) {
        let mut dead_short = false;
        let mut dead_long = false;
        if short_touched {
            if self.short_crs.contains_key(&pledge, self.get_price()) {
                self.short_crs.remove(&pledge, self.get_price());
            }
            if pledge.short.debit > 0 && pledge.short.credit > 0 {
                self.short_crs.insert(&pledge, &(), self.get_price());
            } else {
                dead_short = true;
            }
        }
        if long_touched {
            if self.long_crs.contains_key(&pledge, self.get_price()) {
                self.long_crs.remove(&pledge, self.get_price());
            }
            if pledge.long.debit > 0 && pledge.long.credit > 0 {
                self.long_crs.insert(&pledge, &(), self.get_price());
            } else {
                dead_long = true; // TODO check
            }
        }
        if dead_short && dead_long && (pledge.quid == 0)
        &&  (pledge.near == 0) { self.pledges.remove(id); }
        else { self.pledges.insert(id, pledge); }
    }

    pub(crate) fn stress_pledge(&mut self, id: AccountId) { 
        let mut p: Pledge = self.pledges.get(&id).unwrap(); 
        let mut iVvol = self.get_vol() as f64; // get annualized volatility of NEAR
        let mut short_touched = false;
        let mut long_touched = false;
        let mut due: Balance = 0;         
        let mut cr = computeCR(self.get_price(), p.short.credit, p.short.debit, true);
        if cr < KILL_CR { 
            let nums = self.try_kill_pledge(&p, true);
            p.quid = nums.0; 
            p.near = nums.2;
            p.short.credit = nums.1;
            p.short.debit = nums.3;
            short_touched = true;
        }
        // TODO check precision stuff
        p.stats.short.val_near = self.get_price() * p.short.debit / ONE; // will be sum of each borrowed crypto amt * its price
        let mut val_near = p.stats.short.val_near as f64;
        let mut qd: f64 = p.short.credit as f64;
        if val_near > 0.0 { // $ value of Pledge' NEAR debt
            let mut iW: f64 = val_near; // the amount of this crypto in the user's short debt portfolio, times the crypto's price
            iW /= val_near; // 1.0 for now...TODO later each crypto will carry a different weight in the portfolio
            let var: f64 = (iW * iW) * (iVvol * iVvol); // aggregate for all Pledge's crypto debt
            let mut vol: f64 = var.sqrt(); // portfolio volatility of the Pledge's borrowed crypto
            short_touched = true;

            // $ value of borrowed crypto in upward price shocks of avg & bad magnitudes
            let mut pct: f64 = stress(true, vol, true);
            let avg_val: f64 = (1.0 + pct) * val_near;
            pct = stress(false, vol, true);
            let stress_val: f64 = (1.0 + pct) * val_near;
            let mut stress_loss: f64 = stress_val - qd; // stressed value
    
            if stress_loss < 0.0 {
                stress_loss = 0.0; // better if this is zero, if it's not 
                // that means liquidation (debt value worth > QD collat)
            } 
            let mut avg_loss: f64 = avg_val - qd;
            if avg_loss < 0.0 {   avg_loss = 0.0;   }

            // stats.short.stress_loss += stress_loss; 
            p.stats.short.stress_loss = stress_loss;
            // stats.short.avg_loss += avg_loss; 
            p.stats.short.avg_loss = avg_loss;

            vol *= self.data_s.scale; // market determined implied volaility
            let delta: f64 = pct + 1.0;
            let ln: f64 = delta.ln() * self.data_s.scale; // * calibrate
            let i_stress: f64 = ln.exp() - 1.0;
            let mut payoff: f64 = val_near * (1.0 + i_stress);
            if payoff > qd {
                payoff -= qd;
            } else {
                payoff = 0.0;
            };
            p.stats.short.rate = price(
                payoff, self.data_s.scale, val_near, qd, vol, true
            );
            p.stats.short.premiums = p.stats.short.rate * val_near;
            self.stats.short.premiums += p.stats.short.premiums;
            due = (p.stats.short.premiums / PERIOD).round() as Balance;
            
            p.short.credit = p.short.credit // the user pays their due by losing a bit of QD collateral
                .checked_sub(due).expect(ERR_SUB);
            
            self.live.short.credit = self.live.short.credit // reduce QD collateral in the LivePool
                .checked_sub(due).expect(ERR_SUB);
            
            // TODO scale for this
            let gf: Balance = due.checked_div(11).expect(ERR_DIV);
            due -= gf; // decrement from what is being paid into the gfund pool 
            self.gfund.short.credit = self.gfund.short.credit
                .checked_add(gf).expect(ERR_ADD);
            
            // pay SolvencyProviders by reducing how much they're owed to absorb in QD debt
            if self.dead.long.credit > due { 
                self.dead.long.credit -= due;
            } else { // take the remainder and add it to QD collateral to be absorbed from DeadPool
                due -= self.dead.long.credit;
                self.dead.long.credit = 0;
                self.dead.short.debit = self.dead.short.debit
                    .checked_add(due).expect(ERR_ADD);
            }     
        }     
        cr = computeCR(self.get_price(), p.long.credit, p.long.debit, false);
        if cr < KILL_CR { 
            let nums = self.try_kill_pledge(&p, false);
            p.near = nums.0; 
            p.quid = nums.2;
            p.long.credit = nums.1;
            p.long.debit = nums.3;
            long_touched = true;         
        }
        // TODO unsafe
        p.stats.long.val_near = self.get_price() * p.long.credit / ONE; // will be sum of each crypto collateral amt * its price
        val_near = p.stats.long.val_near as f64;
        qd = p.long.debit as f64;
        if val_near > 0.0 {
            let mut iW: f64 = val_near; // the amount of this crypto in the user's long collateral portfolio, times the crypto's price
            iW /= val_near; // 1.0 for now...TODO later each crypto will carry a different weight in the portfolio
            let var: f64 = (iW * iW) * (iVvol * iVvol); // aggregate for all Pledge's crypto collateral
            let mut vol: f64 = var.sqrt(); // total portfolio volatility of the Pledge's crypto collateral
            long_touched = true;

            // $ value of crypto collateral in downward price shocks of bad & avg magnitudes
            let mut pct: f64 = stress(true, vol, false);
            let avg_val: f64 = (1.0 - pct) * val_near;
            pct = stress(false, vol, false);
            let stress_val: f64 = (1.0 - pct) * val_near;
            
            let mut stress_loss: f64 = qd - stress_val;
            if stress_loss < 0.0 {   stress_loss = 0.0;  }
            let mut avg_loss: f64 = qd - avg_val;
            if avg_loss < 0.0 {   avg_loss = 0.0;   }

            // stats.long.stress_loss += stress_loss; 
            p.stats.long.stress_loss = stress_loss;
            // stats.long.avg_loss += avg_loss; 
            p.stats.long.avg_loss = avg_loss;
            
            vol *= self.data_l.scale; // market determined implied volaility
            let delta: f64 = (-1.0 * pct) + 1.0;
            let ln: f64 = delta.ln() * self.data_l.scale; // calibrate
            let i_stress: f64 = -1.0 * (ln.exp() - 1.0);
            let mut payoff: f64 = val_near * (1.0 - i_stress);
            if payoff > qd {
                payoff = 0.0;
            } else {
                payoff = p.long.debit as f64 - payoff;
            };
            p.stats.long.rate = price(payoff, self.data_l.scale, val_near, qd, vol, false);
            p.stats.long.premiums = p.stats.long.rate * qd;
            self.stats.long.premiums += p.stats.long.premiums;
            due = (p.stats.short.premiums / PERIOD).round() as Balance;
            let mut due_in_near = ratio(1, self.get_price(), due);
            
            p.long.credit = p.long.credit // A Pledge's long side is credited with NEAR collateral
                .checked_sub(due_in_near).expect(ERR_SUB);
            
            self.live.long.credit = self.live.long.credit
                .checked_sub(due_in_near).expect(ERR_SUB);

            let gf: Balance = due_in_near.checked_div(11).expect(ERR_DIV);
            due_in_near -= gf;
            self.gfund.long.credit = self.gfund.long.credit
                .checked_add(gf).expect(ERR_ADD);
            
            // pay SolvencyProviders by reducing how much they're owed to absorb in NEAR debt
            if self.dead.short.credit > due_in_near { 
                self.dead.short.credit -= due_in_near;
            } else { // take the remainder and add it to NEAR collateral to be absorbed from DeadPool
                due_in_near -= self.dead.short.credit;
                
                self.dead.short.credit = 0;
                self.dead.long.debit = self.dead.long.debit
                    .checked_add(due_in_near).expect(ERR_ADD);
            }  
        }
        self.save_pledge(&id, &mut p, long_touched, short_touched);
    }
}
