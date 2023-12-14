use crate::*;


use near_sdk::{env, log, Balance, Promise};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::json_types::{WrappedBalance, WrappedTimestamp, U128};


#[derive(Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct PoolStats {
    pub blood_credit: U128,
    pub blood_debit: U128,
    pub gfund_long_credit: U128,
    pub gfund_long_debit: U128,
    pub gfund_short_credit: U128,
    pub gfund_short_debit: U128,
    pub live_short_credit: U128,
    pub live_short_debit: U128,
    pub live_long_credit: U128,
    pub live_long_debit: U128,
    pub dead_short_credit: U128,
    pub dead_short_debit: U128,
    pub dead_long_credit: U128,
    pub dead_long_debit: U128
} impl PoolStats { 
    pub fn new(c: &Contract) -> Self {
        Self {
            blood_credit: U128::from(c.blood.credit),
            blood_debit: U128::from(c.blood.debit),
            gfund_long_credit: U128::from(c.gfund.long.credit),
            gfund_long_debit: U128::from(c.gfund.long.debit),
            gfund_short_credit: U128::from(c.gfund.short.credit),
            gfund_short_debit: U128::from(c.gfund.short.debit),
            live_short_credit: U128::from(c.live.short.credit),
            live_short_debit: U128::from(c.live.short.debit),
            live_long_credit: U128::from(c.live.long.credit),
            live_long_debit: U128::from(c.live.long.debit),
            dead_short_credit: U128::from(c.dead.short.credit),
            dead_short_debit: U128::from(c.dead.short.debit),
            dead_long_credit: U128::from(c.dead.long.credit),
            dead_long_debit: U128::from(c.dead.long.debit)
        }
    }
}

#[serde(crate = "near_sdk::serde")]
#[derive(BorshDeserialize, BorshSerialize, Debug, Clone, Serialize)]
pub struct Pod { // Used in all Pools, and in individual users' Pledges
    pub credit: Balance, // amount of QD collateral in shorts, NEAR in longs
    pub debit: Balance // amount of QDebt in longs, NEAR debt in shorts
} impl Pod { 
    pub fn new(ins: Balance, outs: Balance) -> Self {
        Self { 
            credit: ins, 
            debit: outs 
        } 
    }
    pub fn clone(&self) -> Self {
        Self {
            credit: self.credit, 
            debit: self.debit,
        }
    }
}

#[serde(crate = "near_sdk::serde")]
#[derive(BorshDeserialize, BorshSerialize, Debug, Serialize)]
pub struct Pool { // Pools have a long Pod and a short Pod
    pub long: Pod, // debt and collateral of QD borrowers
    pub short: Pod, // debt and collateral of NEAR borrowers
} impl Pool { // There are 4 capital pools in QuiD Protocol
    pub fn new() -> Self {
        Self {
            long: Pod::new(0, 0),
            short: Pod::new(0, 0)
        }
    }
}

#[near_bindgen]
impl Contract 
{    
    #[payable]
    // add collateral to LivePool / deposits to SolvencyPool
    // attach a deposit for adding NEAR, amount's for adding QD
    pub fn deposit(&mut self, qd_amt: U128, live: bool) {
        // TODO change vote
        assert!(self.crank.done, "Update in progress");
        let deposit = env::attached_deposit();
        assert!(deposit > 0, ERR_AMT_TOO_LOW);
        let mut amt: Balance = qd_amt.into();
        let mut left = amt;
        let mut min: Balance;
        
        let account = env::predecessor_account_id();
        let mut pledge = self.fetch_pledge(&account, true);
        let mut long_touched = false; let mut short_touched = false;

        // TODO if live = true && no borrowing position open

        if deposit > 1 {
            if live { // adding NEAR collateral to borrower's long position
                long_touched = true;
                pledge.long.credit = pledge.long.credit
                    .checked_add(deposit).expect(ERR_ADD);

                self.live.long.credit = self.live.long.credit
                    .checked_add(deposit).expect(ERR_ADD);
            } 
            else { // adding NEAR deposit to user's SolvencyPool position
                pledge.near = pledge.near
                    .checked_add(deposit).expect(ERR_ADD);

                self.blood.debit = self.blood.debit // crypto can be loaned out as debit to SP
                    .checked_add(deposit).expect(ERR_ADD);            
            }
        }
        if amt > 0 {
            let liq_qd: Balance = self.token.ft_balance_of(
                ValidAccountId::try_from(account.clone()).unwrap()
            ).into();
            min = std::cmp::min(liq_qd, amt);
            if min > 0 {
                self.token.internal_withdraw(&account, min);
                self.token.internal_deposit(&env::current_account_id(), min);
                left -= min;
            }
            if left > 0 {
                min = std::cmp::min(left, pledge.quid);
                left -= min;
                pledge.quid -= min;
            }
            amt -= left;
            if amt > 0 {    
                if live { // adding QD collateral to borrower's short position
                    short_touched = true;
                    pledge.short.credit = pledge.short.credit
                        .checked_add(amt).expect(ERR_ADD);
                    self.live.short.credit = self.live.short.credit
                        .checked_add(amt).expect(ERR_ADD);
                } 
                else { // adding QD deposit to user's SolvencyPool position
                    pledge.quid = pledge.quid
                        .checked_add(amt).expect(ERR_ADD);
                    self.blood.credit = self.blood.credit
                        .checked_add(amt).expect(ERR_ADD);
                }
            }
        }
        self.save_pledge(&account, &mut pledge, long_touched, short_touched);
    }

    // Invoked a la github.com/Narwallets/meta-pool-heartbeat
    // Script must call it regularly, it drives stress testing,
    // and re-pricing options for borrowers on account of this, 
    // and SolvencyTarget as SP's weighted-median voting concedes
    pub fn update(&mut self) {
        if !self.crank.done {
            // let mut pledges = &mut self.pledges; // BUG inside loop throws 
            // "cannot borrow `*self` as mutable more than once at a time"
            let mut keys = &self.pledges.to_vec();
            let len = self.pledges.len() as usize;
            let start = self.crank.index.clone();
            let left: usize = len - start;
            let mut many = 42; // arbitrary number of Pledges to iterate at a time
            // limited by maximum gas that can be burned in one transaction call
            if 42 > left {    many = left;    }
            let stop = start + many;
            for idx in start..stop { 
                let id = keys[idx].0.clone();
                self.stress_pledge(id);
                self.crank.index += 1;
            }
            if self.crank.index == len {
                self.crank.index = 0;
                self.crank.done = true;
                self.crank.last = env::block_timestamp();
            }  
        } else {
            let timestamp = env::block_timestamp();
            let time_delta = timestamp - self.crank.last;
            if time_delta >= EIGHT_HOURS {
                self.crank.done = false;
                let price = self.get_price();
                self.stats.val_near_sp = self.blood.debit.checked_mul(price).expect(
                    "Multiplication Overflow in `update`"
                );
                self.stats.val_total_sp = self.blood.credit
                    .checked_add(self.stats.val_near_sp).expect(ERR_ADD);
                self.sp_stress(None, false); // stress the long side of the SolvencyPool
                self.sp_stress(None, true); // stress the short side of the SolvencyPool
                self.risk(false); self.risk(true); // calculate solvency and scale factor 
            } else {
                env::panic(b"Too early to run an update, please wait"); 
            }
        }
    }  

    pub(crate) fn sp_stress(&mut self, maybe_id: Option<AccountId>, short: bool) -> f64 {
        let ivol = self.get_vol() as f64;
        let price = self.get_price();
        let mut global = true;
        let mut iW: f64 = 0.0; 
        let mut jW: f64 = 0.0;
        if self.stats.val_near_sp > 0 {
            iW = self.stats.val_near_sp
                .checked_div(self.stats.val_total_sp).expect(ERR_DIV) as f64;
     
            jW = self.blood.credit
                .checked_div(self.stats.val_total_sp).expect(ERR_DIV) as f64;
        }
        if let Some(id) = maybe_id {
            global = false;
            if let Some(p) = self.pledges.get(&id) {
                let p_near_val = p.near
                    .checked_mul(price).expect(ERR_MUL);
            
                let value = p.quid.checked_add(p_near_val).expect(ERR_ADD);

                let mut delta_near: Balance = 0;
                let mut delta_qd: Balance = 0;
                if value > 0 {
                    if self.stats.val_near_sp > 0 {
                        delta_near = self.stats.val_near_sp
                            .checked_sub(p_near_val).expect(ERR_SUB);
                    }
                    if self.blood.credit > 0 {
                        delta_qd = self.blood.credit
                            .checked_sub(p.quid).expect(ERR_SUB);
                    }
                    let delta_val = self.stats.val_total_sp
                        .checked_sub(value).expect(ERR_SUB);
                    
                    iW = delta_near
                        .checked_div(delta_val).expect(ERR_DIV) as f64;

                    jW = delta_qd
                        .checked_div(delta_val).expect(ERR_DIV) as f64;
                }            
            }
        }
        let var: f64 = (2.0 * iW * jW * ivol) + (iW * iW * ivol * ivol);
        if var > 0.0 {
            let vol = var.sqrt(); // total volatility of the SolvencyPool
            // % loss that total SP deposits would suffer in a stress event
            let stress_pct = stress(false, vol, short);
            let avg_pct = stress(true, vol, short);
            let mut stress_val: f64 = self.stats.val_total_sp as f64;
            let mut avg_val: f64 = stress_val;
            if !short {
                stress_val *= 1.0 - stress_pct; 
                avg_val *= 1.0 - avg_pct;
                if global {
                    self.stats.long.stress_val = stress_val;
                    self.stats.long.avg_val = avg_val;
                } 
            } else {
                stress_val *= 1.0 + stress_pct;
                avg_val *= 1.0 + avg_pct;
                if global {
                    self.stats.short.stress_val = stress_val;
                    self.stats.short.avg_val = avg_val;
                } 
            }
            return stress_val;
        } else {
            return 0.0;
        }
    }
    
    pub(crate) fn risk(&mut self, short: bool) {
        let mvl_s: f64; // market value of liabilities in stressed markets 
        let mva_s: f64; // market value of assets in stressed markets 
        let mva_n = self.stats.val_total_sp; //market value of insurance assets in normal markets,
        // includes the reserve which is implemented as an insurer, collateral is not an asset of the insurers
        
        let mut vol = self.get_vol() as f64; 
        let val_near: f64;
        if !short {
            val_near = self.live.long.credit
                .checked_mul(self.get_price()).expect(ERR_MUL) as f64;

            let qd: f64 = self.live.long.debit as f64;
            let mut pct: f64 = stress(false, vol, false);
            
            let stress_val = (1.0 - pct) * val_near;
            let stress_loss = qd - stress_val;
            
            mva_s = stress_val; // self.stats.long.stress_val;
            mvl_s = stress_loss; // self.stats.long.stress_loss;
        } else {
            val_near = self.live.short.debit
                .checked_mul(self.get_price()).expect(ERR_MUL) as f64;
            
            let qd: f64 = self.live.short.credit as f64;
            let mut pct: f64 = stress(false, vol, true);
            
            let stress_val = (1.0 + pct) * val_near;
            let stress_loss = stress_val - qd;

            mva_s = stress_val; // self.stats.short.stress_val;
            mvl_s = stress_loss; // self.stats.short.stress_loss;
        }    
        let own_n = mva_n as f64; // own funds normal markets
        let mut own_s = mva_s - mvl_s; // own funds stressed markets
        if short && own_s > 0.0 {
            own_s *= -1.0;   
        }
        // S.olvency C.apial R.equirement is the amount of... 
        // deposited assets needed to survive a stress event
        let scr = own_n - own_s;
        assert!(scr > 0.0, "SCR can't be 0");
        let solvency = own_n / scr; // represents capital adequacy to back $QD
        if short {
            let mut target = self.data_s.median;
            if target == -1.0 {
                target = 1.0;
            }
            // assume target = 1.5 or 150%
            let mut scale = target / solvency;
            if scale > 4.2 {
                scale = 4.2;    
            } else if scale < 0.042 {
                scale = 0.042;
            }
            self.data_s.scale = scale;
            self.data_s.solvency = solvency
        } else {
            let mut target = self.data_l.median;
            if target == -1.0 {
                target = 1.0;
            }
            let mut scale = target / solvency;
            if scale > 4.2 {
                scale = 4.2;    
            } else if scale < 0.042 {
                scale = 0.042;
            }
            self.data_l.scale = scale;
            self.data_l.solvency = solvency;
        }
    }
}