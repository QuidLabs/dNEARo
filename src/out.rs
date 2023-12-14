use crate::*;


use near_sdk::{env, log, Balance, Promise};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::json_types::{WrappedBalance, WrappedTimestamp, U128};

#[near_bindgen]
impl Contract 
{
    #[payable]
    pub fn borrow(&mut self, amount: U128, short: bool) -> PromiseOrValue<U128> { 
        let mut cr: u128; 
        let mut transfer = false;
        
        let mut amt: Balance = amount.into();
        let deposit = env::attached_deposit();
        assert!(self.crank.done, "Update in progress");
        assert!(deposit > 0 && amt > ONE, ERR_AMT_TOO_LOW);
        
        let account = env::predecessor_account_id();
        let mut pledge = self.fetch_pledge(&account, true);
        
        if !short {
            cr = computeCR(self.get_price(), pledge.long.credit, pledge.long.debit, false);
            assert!(cr == 0 || cr >= MIN_CR, "Cannot borrow while your current CR is below minimum");
            if deposit >= ONE {
                pledge.long.credit = pledge.long.credit
                    .checked_add(deposit).expect(ERR_ADD);
               
                self.live.long.credit = self.live.long.credit
                    .checked_add(deposit).expect(ERR_ADD);
            }
            let new_debt = pledge.long.debit 
                .checked_add(amt).expect(ERR_ADD);
            
            //assert!(new_debt >= MIN_DEBT, "Value of debt must be worth above $90 of QD");
            
            cr = computeCR(self.get_price(), pledge.long.credit, new_debt, false);
            if cr >= MIN_CR { // requested amount to borrow is within measure of collateral
                self.mint(&account, amt);
                // TODO pull from GFund (or in mint)
                pledge.long.debit = new_debt;
            } 
            else { // instead of throwing a "below MIN_CR" error right away, try to satisfy loan
                assert!(true, "can't go below MIN CR");
                (self.live.long, pledge.long) = self.valve(account.clone(),
                    false, new_debt, 
                    self.live.long.clone(),
                    pledge.long.clone()
                ); 
            }
        } else { // borrowing short
            if deposit > 1 { /* if they dont have QD and they send in NEAR, 
                we can just immediately invert it and use that as coll */
                self.invert(deposit); // QD value of the NEAR debt being cleared 
                pledge.short.credit = pledge.short.credit.checked_add( // QD value of the NEAR deposit
                    ratio(self.get_price(), deposit, ONE)).expect(ERR_ADD);
            }
            cr = computeCR(self.get_price(), pledge.short.credit, pledge.short.debit, true);
            assert!(cr == 0 || cr >= MIN_CR, "Cannot borrow while your current CR is below minimum"); 
            
            let new_debt = pledge.short.debit
                .checked_add(amt).expect(ERR_ADD);

            let new_debt_in_qd = ratio(self.get_price(), new_debt, ONE);
            
            //assert!(new_debt_in_qd >= MIN_DEBT, "Value of debt must be worth above $90 of QD");
            
            cr = ratio(ONE, pledge.short.credit, new_debt_in_qd);
            if cr >= MIN_CR {
                transfer = true; // when borrowing within their means, we disperse NEAR that the borrower can sell
            } else {
                (self.live.short, pledge.short) = self.valve(account.clone(),
                    true, new_debt_in_qd, 
                    self.live.short.clone(),
                    pledge.short.clone()
                );
            }
        }
        self.save_pledge(&account, &mut pledge, !short, short);
        if transfer { // transfer bool is a workaround for "borrow after move" compile error
            return PromiseOrValue::Promise(Promise::new(account).transfer(amt));
        } 
        return PromiseOrValue::Value(U128(0));
    }

    // TODO make sure that insurers get also have internal accounts not just borrowers 
    pub(crate) fn mint(&mut self, id: &AccountId, amt: u128) { // mint $QD stablecoins
        if self.token.accounts.get(&id).is_some() {
            self.token.internal_deposit(&id, amt);
        } else {
            self.token.internal_register_account(&id);
            self.token.internal_deposit(&id, amt);
        }
    }

    // https://twitter.com/1x_Brasil/status/1522663741023731714
    pub(crate) fn valve(&mut self, id: AccountId, short: bool, new_debt_in_qd: u128, mut live: Pod, mut pledge: Pod) -> (Pod, Pod) {
        let mut check_zero = false;
        let now_liq_qd: Balance = self.token.ft_balance_of(
            ValidAccountId::try_from(id.clone()).unwrap()
        ).into();
        let now_coll_in_qd: Balance;
        let now_debt_in_qd: Balance;
        if short {
            now_debt_in_qd = ratio(self.get_price(), pledge.debit, ONE);
            now_coll_in_qd = pledge.credit; 
        } else {
            now_coll_in_qd = ratio(self.get_price(), pledge.credit, ONE);
            now_debt_in_qd = pledge.debit;   
        }
        let mut net_val: Balance = now_liq_qd
            .checked_add(now_coll_in_qd).expect(ERR_ADD)
            .checked_sub(now_debt_in_qd).expect(ERR_SUB);
        
        let mut fee_amt: Balance = net_val.checked_sub( // (net_val - (1 - 1 / 1.1 = 0.090909...) * col_init) / 11
            ratio(DOT_OH_NINE, now_coll_in_qd, ONE)
        ).expect(ERR_SUB).checked_div(11).expect(ERR_DIV); // = 1 + (1 - 1 / 1.1) / fee_% 
        
        let mut qd_to_buy: Balance = fee_amt // (fee_amt / fee_%) i.e div 0.009090909...
            .checked_mul(110).expect(ERR_MUL);
        let mut end_coll_in_qd: Balance = qd_to_buy
            .checked_add(now_coll_in_qd).expect(ERR_ADD);

        let max_debt = ratio(ONE, end_coll_in_qd, MIN_CR);    
        let final_debt: Balance;
        if new_debt_in_qd >= max_debt {
            final_debt = max_debt;
            check_zero = true;
        } else { // max_debt is larger than the requested debt  
            final_debt = new_debt_in_qd;
            end_coll_in_qd = ratio(MIN_CR, final_debt, ONE);
            qd_to_buy = end_coll_in_qd // no need to mint all this QD, gets partially minted in `redeem`, excluding the
                .checked_sub(now_coll_in_qd).expect(ERR_SUB); // amount cleared against DeadPool's QDebt
            fee_amt = ratio(FEE, qd_to_buy, ONE);
        }
        net_val -= fee_amt;
        self.mint(&env::current_account_id(), fee_amt); // mint fee in QD
        let eleventh = fee_amt.checked_div(11).expect(ERR_DIV);
        
        let rest = fee_amt.checked_sub(eleventh).expect(ERR_SUB);
        self.dead.short.debit = self.dead.short.debit.checked_add(rest).expect(ERR_ADD);
        self.gfund.short.credit = self.gfund.short.credit.checked_add(eleventh).expect(ERR_ADD);
    
        if short {
            pledge.credit = end_coll_in_qd;
            pledge.debit = ratio(ONE, final_debt, self.get_price());
            
            live.credit = live.credit
                .checked_add(qd_to_buy).expect(ERR_ADD);
            let near_to_sell = ratio(ONE, qd_to_buy, self.get_price());
            // NEAR spent on buying QD collateral must be paid back by the borrower to unlock the QD
            live.debit = live.debit
                .checked_add(near_to_sell).expect(ERR_ADD);
            
            // we must first redeem QD that we mint out of thin air to purchase the NEAR, 
            // before burning NEAR debt with it to purchase QD (undoing the mint) collat
            self.redeem(qd_to_buy);
            self.invert(near_to_sell);     
        } else {    
            // get final collateral value in NEAR
            let end_coll = ratio(ONE, end_coll_in_qd, self.get_price());
            pledge.credit = end_coll;
            pledge.debit = final_debt;

            let delta_coll = end_coll
                .checked_sub(pledge.credit).expect(ERR_SUB);
            live.credit = live.credit
                .checked_add(delta_coll).expect(ERR_ADD);
            // QD spent on buying NEAR collateral must be paid back by the borrower to unlock the NEAR
            live.debit = live.debit
                .checked_add(qd_to_buy).expect(ERR_ADD);
            
            /******/ self.redeem(qd_to_buy); /******/
        }
        /*
            Liquid NEAR value in QD
                = (FinalDebt + Net) * (1 - 1.10 / (Net / FinalDebt + 1))
            Net = liquid QD + initial QD collat - initial NEAR debt in QD                  
        */
        let net_div_debt = ratio(
            ONE, net_val, final_debt
        ).checked_add(ONE).expect(ERR_ADD);

        let between = ONE.checked_sub( // `between` must >= 0 as a rule
            ratio(ONE, MIN_CR, net_div_debt)    
        ).expect("Illegal borrow attempt"); 

        let end_liq_qd = ratio(between, 
            final_debt.checked_add(net_val).expect(ERR_ADD),
        ONE);

        assert!(!check_zero || end_liq_qd == 0, "Something went wrong in `borrow");
        
        let delta_liq_qd: i128 = end_liq_qd.try_into().unwrap();
        let mut liq_qd = delta_liq_qd
            .checked_sub(now_liq_qd.try_into().unwrap()).expect(ERR_SUB);
        if liq_qd > 0 {
            self.mint(&id, liq_qd.try_into().unwrap());
        } 
        else if liq_qd < 0 { liq_qd *= -1;
            self.token.internal_withdraw(&id, liq_qd.try_into().unwrap());   
        }
        assert!(computeCR(self.get_price(), pledge.credit, pledge.debit, short) >= MIN_CR, 
        "Cannot do operation that would result in short CR below min"); 
        return (live, pledge);
    }

    /**
     * This function exists to allow withdrawal of deposits, either from 
     * a user's SolvencyPool deposit, or LivePool (borrowing) position.
     * Hence, the first boolean parameter's for indicating which pool,
     * & last boolean parameter indicates the currency being withdrawn.
     * @param sp = SolvencyPool
     */
    #[payable]
    pub fn renege(&mut self, amount: U128, sp: bool, qd: bool) -> PromiseOrValue<U128> {
        // TODO change vote
        assert!(self.crank.done, "Update in progress");
        assert_one_yocto();
        
        let amt: Balance = amount.into();
        assert!(amt > ONE, ERR_AMT_TOO_LOW);
        
        let cr: u128; let mut min: u128; 
        let mut transfer: bool = false;
        
        let account = env::predecessor_account_id();
        let mut pledge = self.fetch_pledge(&account, false);
        
        let all_qd: Balance = self.token.ft_balance_of(
            ValidAccountId::try_from(env::current_account_id()).unwrap()).into();

        let mut fee = ratio(FEE, amt, ONE);
        let mut amt_sub_fee = amt.checked_sub(fee).expect(ERR_SUB);
        let gf_cut = fee.checked_div(11).expect(ERR_DIV);
        fee -= gf_cut;

        if !sp { // we are withdrawing collateral from a borrowing position
            if qd {
                pledge.short.credit = pledge.short.credit.checked_sub(amt).expect(ERR_SUB);
                cr = computeCR(self.get_price(), pledge.short.credit, pledge.short.debit, true);
                assert!(cr >= MIN_CR, ERR_BELOW_MIN_CR);

                min = std::cmp::min(all_qd, amt_sub_fee); // maximum dispensable QD
                if amt_sub_fee > min { // there's not enough QD in the contract to send
                    amt_sub_fee -= min; // remainder to be gfundn as...
                    self.gfund.long.debit = self.gfund.long.debit // ...protocol debt  
                        .checked_add(amt_sub_fee).expect(ERR_ADD);
                }
                self.token.internal_deposit(&account, amt_sub_fee); // send QD to the signer
                self.token.internal_withdraw(&env::current_account_id(), min);
                self.live.short.credit = self.live.short.credit.checked_sub(amt).expect(ERR_SUB);
                
                self.dead.short.debit = self.dead.short.debit.checked_add(fee).expect(ERR_ADD); // pay fee
                self.gfund.short.credit = self.gfund.short.credit.checked_add(gf_cut).expect(ERR_ADD);
            }
            else {
                transfer = true; // we are sending NEAR to the user
                pledge.long.credit = pledge.long.credit.checked_sub(amt).expect(ERR_SUB);
                cr = computeCR(self.get_price(), pledge.long.credit, pledge.long.debit, false);
                assert!(cr >= MIN_CR, ERR_BELOW_MIN_CR);
                let near = env::account_balance();
                if amt_sub_fee > near { // there's not enough NEAR in the contract to send
                    let in_qd = ratio(self.get_price(), amt_sub_fee - near, ONE);
                    amt_sub_fee = near;
                    self.token.internal_deposit(&account, in_qd); // mint requested QD
                    self.gfund.long.debit = self.gfund.long.debit // freeze as protocol debt  
                        .checked_add(in_qd).expect(ERR_ADD);
                }
                self.live.long.credit = self.live.long.credit.checked_sub(amt).expect(ERR_SUB);
                self.dead.long.debit = self.dead.long.debit.checked_add(fee).expect(ERR_ADD);
                self.gfund.long.credit = self.gfund.long.credit.checked_add(gf_cut).expect(ERR_ADD);
            }   
        } else { // we are withdrawing deposits from the SolvencyPool
            let mut remainder;
            if qd {
                pledge.quid = pledge.quid.checked_sub(amt).expect(ERR_SUB);
                min = std::cmp::min(self.blood.credit, amt); // maximum dispensable QD
                self.blood.credit -= min;
                remainder = amt - min;
                if remainder > 0 {
                    min = std::cmp::min(self.gfund.short.credit, remainder);
                    self.gfund.short.credit -= min;
                    if remainder > min {
                        remainder -= min;
                        self.gfund.long.debit = self.gfund.long.debit
                            .checked_add(remainder).expect(ERR_ADD);      
                    }
                }
                self.token.internal_withdraw(&env::current_account_id(), amt_sub_fee); 
                self.token.internal_deposit(&account, amt_sub_fee); // send QD to the signer
                self.dead.short.debit = self.dead.short.debit.checked_add(fee).expect(ERR_ADD); // pay fee
                self.gfund.short.credit = self.gfund.short.credit.checked_add(gf_cut).expect(ERR_ADD);
            } else {
                transfer = true;
                pledge.near = pledge.near.checked_sub(amt).expect(ERR_SUB);
                min = std::cmp::min(self.blood.debit, amt); // maximum dispensable NEAR
                self.blood.debit -= min;
                remainder = amt - min;
                if remainder > 0 {
                    min = std::cmp::min(self.gfund.long.credit, remainder); // maximum dispensable NEAR
                    self.gfund.long.credit -= min;
                    if remainder > min {
                        remainder -= min;
                        amt_sub_fee -= remainder;
                        let in_qd = ratio(self.get_price(), remainder, ONE);
                        self.token.internal_deposit(&account, in_qd); // mint requested QD
                        self.gfund.long.debit = self.gfund.long.debit // freeze as protocol debt  
                            .checked_add(in_qd).expect(ERR_ADD);
                    }
                }
                self.dead.long.debit = self.dead.long.debit.checked_add(fee).expect(ERR_ADD); // pay fee
                self.gfund.long.credit = self.gfund.long.credit.checked_add(gf_cut).expect(ERR_ADD);
            }
        }
        self.save_pledge(&account, &mut pledge, !sp && !qd, !sp && qd);
        if transfer { // workaround for "borrow after move" compile error
            return PromiseOrValue::Promise(Promise::new(account).transfer(amt_sub_fee));
        }
        return PromiseOrValue::Value(U128(0));
    }

    // Close out caller's borrowing position by paying
    // off all pledge's own debt with own collateral
    #[payable]
    pub fn fold(&mut self, short: bool) { 
        assert_one_yocto();
        let id = env::predecessor_account_id();
        let mut pledge = self.fetch_pledge(&id, false);
        if short {
            let cr = computeCR(self.get_price(), pledge.short.credit, pledge.short.debit, true);
            if cr > KILL_CR { // mainly a sanity check, an underwater pledge will almost certainly
                // take QD and sell it for NEAR internally in the interest of proper accounting
                let qd = ratio(self.get_price(), pledge.short.debit, ONE);
                self.redeem(qd); // https://youtu.be/IYXRSR0xNVc?t=111 pledges will probably
                // be clipped before its owner can fold in time to prevent that from occuring...
                self.turn( pledge.short.debit, false, true, &mut pledge);
            }
        } else {
            let cr = computeCR(self.get_price(), pledge.long.credit, pledge.long.debit, false);
            if cr > KILL_CR {
                let near = ratio(ONE, pledge.long.debit, self.get_price());
                self.invert(near);
                self.turn(pledge.long.debit, false, false, &mut pledge);
            }
        }   
    }
}