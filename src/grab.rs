use crate::*;


use near_sdk::{env, log, Balance, Promise};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::json_types::{WrappedBalance, WrappedTimestamp, U128};

#[near_bindgen]
impl Contract 
{
    #[payable]
    pub fn swap(&mut self, amount: U128, repay: bool, short: bool) { // TODO rename 
        let mut amt: Balance = amount.into();
        let deposit = env::attached_deposit();
        let account = env::predecessor_account_id();
        assert!(self.crank.done, "Update in progress");
        assert!(deposit > 0, ERR_AMT_TOO_LOW);
        if !repay {
            if short { // NEAR ==> QD (short collat), AKA inverting NEAR debt
                assert!(deposit >= ONE, ERR_AMT_TOO_LOW);
                // TODO if account == richtobacco.near
                // do invertFrom
                self.invert(deposit);

                let mut quid = ratio(self.get_price(), deposit, ONE);        
                let mut fee_amt = ratio(FEE, quid, ONE);
                // https://www.youtube.com/watch?v=KoIqcDZ5ewY
                
                let gf_cut = fee_amt.checked_div(11).expect(ERR_DIV);
                self.gfund.short.credit = self.gfund.short.credit
                    .checked_add(gf_cut).expect(ERR_ADD);
                    
                quid -= fee_amt;
                fee_amt -= gf_cut;

                self.dead.short.debit = self.dead.short.debit
                    .checked_add(fee_amt).expect(ERR_ADD);
                
                self.token.internal_deposit(&account, quid);
            } 
            else { // QD ==> NEAR (long collat), AKA redeeming $QDebt 
                assert!(amt >= ONE, ERR_AMT_TOO_LOW);
                
                self.redeem(amt);
                self.token.internal_withdraw(&account, amt); // burn the QD being sold 
                let mut near = ratio(ONE, amt, self.get_price());
                let mut fee_amt = ratio(FEE, near, ONE);
            
                let gf_cut = fee_amt.checked_div(11).expect(ERR_DIV);
                self.gfund.long.credit = self.gfund.long.credit 
                    .checked_add(gf_cut).expect(ERR_ADD);
                
                near -= fee_amt;
                fee_amt -= gf_cut;
                
                self.dead.long.debit = self.dead.long.debit
                    .checked_add(fee_amt).expect(ERR_ADD);

                Promise::new(account).transfer(near); // send NEAR to redeemer
            }    
        } else { // decrement caller's NEAR or QDebt without releasing collateral
            let mut pledge = self.fetch_pledge(&account, false);
            if !short { // repay QD debt, distinct from premium payment which does not burn debt but instead distributes payment
                self.token.internal_withdraw(&account, amt); // burn the QD being paid in as premiums 
                self.turn(amt, true, false, &mut pledge);
            }
            else { // repay NEAR debt, distinct from premium payment (see previous comment next to `else if`)
                assert!(deposit > 1, ERR_AMT_TOO_LOW);
                self.turn(deposit, true, true, &mut pledge);
            }
        }
    }

    /**
     * The second act is called "The Turn". The magician takes the ordinary 
     * something and makes it do something extraordinary. Now you're looking
     * for the secret...but you won't find it, because of course you're not
     * really looking...you don't really want to know, you wanna be fooled, but
     * you wouldn't clap yet...because makin' somethin' disappear ain't enough  
     */
     pub(crate) fn turn(&mut self, amt: u128, 
                        repay: bool, short: bool,
                        pledge: &mut Pledge) -> Balance {
        let min: Balance;
        let id = pledge.id.clone();
        if !short { // burn QD up to the pledge's total long debt
            min = std::cmp::min(pledge.long.debit, amt);
            if min > 0 { // there is any amount of QD debt to burn
                pledge.long.debit -= min;
                self.live.long.debit = self.live.long.debit
                    .checked_sub(min).expect(ERR_SUB);
            }
        } 
        else { // burn NEAR debt
            min = std::cmp::min(pledge.short.debit, amt);
            if min > 0 {
                pledge.short.debit -= min;
                self.live.short.debit = self.live.short.debit
                    .checked_sub(min).expect(ERR_SUB);
            }
        }
        if min > 0 { // the Pledge was touched
            if !repay { 
                if !short { // release NEAR collateral as a consequence of redeeming debt
                    let redempt = ratio(KILL_CR, min, self.get_price());
                    
                    pledge.long.credit = pledge.long.credit
                        .checked_sub(redempt).expect(ERR_SUB);
                    
                    self.live.long.credit = self.live.long.credit
                        .checked_sub(redempt).expect(ERR_SUB);
                } 
                else { // release QD collateral...
                    // how much QD is `min` worth
                    let redempt = ratio(self.get_price(), min, ONE);
                        
                    pledge.short.credit = pledge.short.credit
                        .checked_sub(redempt).expect(ERR_SUB);
    
                    self.live.short.credit = self.live.short.credit
                        .checked_sub(redempt).expect(ERR_SUB);
                }
            } 
            self.save_pledge(&id, pledge, !short, short); 
        } 
        return min; // how much was redeemed, used for total tallying in turnFrom 
    }

    /*
     * loop through active Pledges in CR range 100-110%
     * by sorted order of increasing CR (lowest CR first),
     * through a composite key that prioritizes higher debt,
     * and burn from their QD/NEAR debt, while withdrawing 
     * equal value in NEAR/QD collateral to send to invoker
     */ 
    pub(crate) fn turnFrom(&mut self, mut amt: u128, short: bool, many: usize) -> Balance {
        // TODO what if there's not even `many` elements to iterate?
        if !short { // amt is interpreted as QD debt
            // TODO if many > the length 
            for (mut pledge, _) in self.long_crs.get_top(many) {
                if amt > 0 {
                    let CR = pledge.get_CR(false, self.get_price());
                    // For liquidation (full via `clip` or partial from
                    // within invert/redeem) we are only interested in 
                    // pledges between 90-100 LTV CR, since the map is 
                    // sorted we can break the loop on the first LTV<90
                    if CR.0 >= MIN_CR { break; }
                    else if CR.0 < KILL_CR { continue; }
                    // this is a rare edge case as 'clip bots'
                    // will continuously churn liquidatables
                    // by reading from the head of this map
                    // and calling `clip` on risky pledges
                }
                amt -= self.turn(amt, false, false, &mut pledge); // burn QD debt from pledge
            }
        } else { // amt is interpreted as NEAR debt 
            for (mut pledge, _) in self.short_crs.get_top(many) { 
                if amt > 0 {
                    let CR = pledge.get_CR(true, self.get_price());
                    if CR.0 >= MIN_CR { break; }
                    // TODO this fails by having big pledges on top, never reaching
                    // small Pledges with CRs above 90 because the magnitude of 
                    // big Pledges will just overtake their index in the treemap
                    else if CR.0 < KILL_CR { continue; }
                    // No need to skip the originator of the redemption/inversion
                    // if they are in the 100-110 range who cares if someone else
                    // partially liquidates them or if they do it to themselves
                    amt -= self.turn(amt, false, true, &mut pledge); // burn NEAR debt from pledge
                } else { break; }
            }
        }
        return amt; // remaining amount to redeem
    }

    // pub(crate) fn redeemFrom(&mut self, quid: Balance) {
    //     // TODO move turnFrom piece here and let `update` bot handle this using GFund for liquidity
    // }
    pub(crate) fn redeem(&mut self, quid: Balance) {
        let bought: Balance; // NEAR collateral to be released from DeadPool's long portion
        let mut redempt: Balance = 0; // amount of QD debt being cleared from the DP
        let mut amt = self.turnFrom(quid, false, 10); // TODO 10 hardcoded
        if amt > 0 {  // fund redemption by burning against pending DP debt
            let mut val_collat = ratio(self.get_price(), self.dead.long.debit, ONE);
            if val_collat > self.dead.long.credit { // QD in DP worth less than NEAR in DP
                val_collat = self.dead.long.credit; // max QDebt amount that's clearable 
                // otherwise, we can face an edge case where tx throws as a result of
                // not being able to draw equally from both sides of DeadPool.long
            } if val_collat >= amt { // there is more QD in the DP than amt sold
                redempt = amt; 
                amt = 0; // there will be 0 QD left to clear
            } else {
                redempt = val_collat;
                amt -= redempt; // we'll still have to clear some QD
            }
            if redempt > 0 {
                // NEAR's worth of the QD we're about to displace in the DeadPool
                bought = ratio(ONE, redempt, self.get_price());
                // paying the DeadPool's long side by destroying QDebt
                self.dead.long.credit = self.dead.long.credit
                    .checked_sub(redempt).expect(ERR_SUB);
                self.dead.long.debit = self.dead.long.debit
                    .checked_sub(bought).expect(ERR_SUB);
            }
            if amt > 0 { // there is remaining QD to redeem after redeeming from DeadPool  
                let mut near = ratio(ONE, amt, self.get_price());
                assert!(env::account_balance() > near, 
                    "Insufficient NEAR in the contract to clear this redemption"
                );
                let mut min = std::cmp::min(self.blood.debit, near); // maximum NEAR dispensable by SolvencyPool
                amt = ratio(self.get_price(), amt, ONE); // QD paid to SP for NEAR sold 
                self.token.internal_deposit(&env::current_account_id(), amt);
                self.blood.credit // offset, in equal value, the NEAR sold by SP
                    .checked_add(amt).expect(ERR_ADD);
                self.blood.debit -= min; // sub NEAR that's getting debited out of the SP
                near -= min;
                if near > 0 { // hint, das haben eine kleine lobstah boobie 
                    amt = ratio(self.get_price(), near, ONE); // in QD
                    self.token.internal_deposit(&env::current_account_id(), amt);
                    // DP's QD will get debited (canceling NEAR debt) in inversions
                    self.dead.short.debit = self.dead.short.debit
                        .checked_add(amt).expect(ERR_ADD);
                    // append defaulted NEAR debt to the DP as retroactive settlement
                    self.dead.short.credit = self.dead.short.credit
                        .checked_add(near).expect(ERR_ADD);   
                }
            }
        }
    }

    // pub(crate) fn invertFrom(&mut self, quid: Balance) {
    //     // TODO move turnFrom piece here and let `update` bot handle this using GFund for liquidity
    // }
    pub(crate) fn invert(&mut self, near: Balance) {
        let mut bought: Balance = 0; // QD collateral to be released from DeadPool's short portion
        let mut redempt: Balance = 0; // amount of NEAR debt that's been cleared from DP
        // invert against LivePool, `true` for short, returns NEAR remainder to invert
        let mut amt = self.turnFrom(near, true, 10); // TODO 10 hardcoded
        if amt > 0 { // there is remaining NEAR to be bought 
            // can't clear more NEAR debt than is available in the DeadPool
            let mut val = std::cmp::min(amt, self.dead.short.credit);
            val = ratio(self.get_price(), val, ONE); // QD value
            log!("vaaalll...{}", val);
            if val > 0 && self.dead.short.debit >= val { // sufficient QD collateral vs value of NEAR sold
                log!("self.dead.short.debit...{}", self.dead.short.debit);
                redempt = amt; // amount of NEAR credit to be cleared from the DeadPool
                log!("redempt...{}", redempt);
                bought = val; // amount of QD to debit against short side of DeadPool
                log!("bought...{}", bought);
                amt = 0; // there remains no NEAR debt left to clear in the inversion
            } else if self.dead.short.debit > 0 { // there is less NEAR credit to clear than the amount being redeemed
                bought = self.dead.short.debit; // debit all QD collateral in the DeadPool
                redempt = ratio(ONE, bought, self.get_price());
                amt -= redempt;
            }
            if redempt > 0 {
                log!("redempt...{}", redempt);
                self.dead.short.credit = self.dead.short.credit // NEAR Debt
                    .checked_sub(redempt).expect(ERR_SUB);
                self.dead.short.debit = self.dead.short.debit // QD Collat
                    .checked_sub(bought).expect(ERR_SUB);
            }
            if amt > 0 { // remaining NEAR to redeem after clearing against LivePool and DeadPool
                let mut quid = ratio(self.get_price(), amt, ONE);
                let liq_qd: Balance = self.token.ft_balance_of(
                    ValidAccountId::try_from(env::current_account_id()).unwrap()).into();
                assert!(liq_qd > quid, "Insufficient QD in the contract to clear this inversion");
                
                let min = std::cmp::min(quid, self.blood.credit);
                let min_near = ratio(ONE, min, self.get_price());
                self.token.internal_withdraw(&env::current_account_id(), min);
                self.blood.debit = self.blood.debit
                    .checked_add(min_near).expect(ERR_ADD);
                self.blood.credit -= min;
                amt -= min_near;
                quid -= min;
                if quid > 0 { // und das auch 
                    // we credit NEAR to the long side of the DeadPool, which gets debited when redeeming QDebt
                    self.dead.long.debit = self.dead.long.debit
                        .checked_add(amt).expect(ERR_ADD);
                    // append defaulted $QDebt to the DeadPool as retroactive settlement to withdraw SPs' $QD
                    self.dead.long.credit = self.dead.long.credit
                        .checked_add(quid).expect(ERR_ADD);
                    // TODO how come we don't get to print when we do this?
                }
            }
        }
    }
}