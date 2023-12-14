use crate::*;


use near_sdk::{env, log, Balance, Promise};
use near_sdk::serde::{Deserialize, Serialize};
use near_sdk::json_types::{WrappedBalance, WrappedTimestamp, U128};

#[near_bindgen]
impl Contract 
{    
    // QuiD's bot script will continuously call this liquidation function on distressed 
    // Pledges. For this reason there is no liquidation fee, because there is an implicit
    // incentive for anyone to run this function, otherwise the peg will be destroyed.
    pub fn clip(&mut self, account: ValidAccountId) { 
        assert_one_yocto();
        let mut long_touched = false;
        let mut short_touched = false;
        let id: AccountId = account.clone().into();
        // We don't use fetch_pledge because we'd rather not absorb into
        // a pledge until after they are rescued, to keep their SP balances
        // as high as possible in the interest of rescuing
        if let Some(mut pledge) = self.pledges.get(&id) {
            // TODO clip biggest one first, or the lowest CR first if same size 
            let mut cr = computeCR(self.get_price(), pledge.long.credit, pledge.long.debit, false);
            // TODO if the position is in the user defined range, shrink it
            if cr < MIN_CR {
                let nums = self.try_kill_pledge(&pledge, false);
                pledge.long.credit = nums.1;
                pledge.long.debit = nums.3;
                pledge.near = nums.0;
                pledge.quid = nums.2;
                long_touched = true;
            }
            cr = computeCR(self.get_price(), pledge.short.credit, pledge.short.debit, true);
            if cr < MIN_CR {
                let nums = self.try_kill_pledge(&pledge, true);
                pledge.short.credit = nums.1;
                pledge.short.debit = nums.3;
                pledge.quid = nums.0;
                pledge.near = nums.2;
                short_touched = true;
            }
            self.save_pledge(&id, &mut pledge, long_touched, short_touched);
        }
    }

    pub(crate) fn try_kill_pledge(&mut self, pledge: &Pledge, short: bool) -> (Balance, Balance, Balance, Balance) {
        let available: Balance = self.token.ft_balance_of(
            ValidAccountId::try_from(pledge.id.clone()).unwrap()
        ).into(); 
        let mut old_nums: (Balance, Balance, Balance, Balance);
        let mut nums: (Balance, Balance, Balance, Balance);
        let mut cr: u128;
        /** Liquidation protection does an off-setting where deficit margin (delta from min CR)
        in a Pledge can be covered by either its SP deposit, or possibly (TODO) the opposite 
        borrowing position. However, it's rare that a Pledge will borrow both long & short. **/
        if short {
            old_nums = (
                pledge.quid, pledge.short.credit, 
                pledge.near, pledge.short.debit
            );
            nums = self.short_save(&pledge, available);
            cr = computeCR(self.get_price(), nums.1, nums.3, true);
            if cr < KILL_CR { // we are liquidating this pledge
                // undo asset displacement by short_save
                let now_available: Balance = self.token.ft_balance_of(
                    ValidAccountId::try_from(pledge.id.clone()).unwrap()
                ).into();
                if available > now_available {
                    self.token.internal_withdraw(&pledge.id, available - now_available);
                }
                if old_nums.0 > nums.0 { // SP QD changed
                    let delta = old_nums.0 - nums.0;
                    self.live.short.credit = self.live.short.credit
                        .checked_sub(delta).expect(ERR_SUB);
                    self.blood.credit = self.blood.credit
                        .checked_add(delta).expect(ERR_ADD);
                }
                if old_nums.2 > nums.2 { // SP NEAR changed
                    let delta = old_nums.2 - nums.2;
                    self.live.short.debit = self.live.short.debit
                        .checked_add(delta).expect(ERR_ADD);
                    self.blood.debit = self.blood.debit
                        .checked_add(delta).expect(ERR_ADD);
                }
                // move liquidated assets from LivePool to DeadPool
                self.snatch(nums.3, nums.1, true);
                return (old_nums.0, 0, old_nums.2, 0); // zero out the pledge
            } else if cr < MIN_CR {
                (nums.1, nums.3) = self.shrink(nums.1, nums.3, true);
            }
        } else {
            old_nums = (
                pledge.near, pledge.long.credit, 
                pledge.quid, pledge.long.debit
            );
            nums = self.long_save(&pledge, available);
            cr = computeCR(self.get_price(), nums.1, nums.3, false);
            if cr < KILL_CR {
                let now_available: Balance = self.token.ft_balance_of(
                    ValidAccountId::try_from(pledge.id.clone()).unwrap()
                ).into();
                if available > now_available {
                    self.token.internal_withdraw(&pledge.id, available - now_available);
                }
                if old_nums.0 > nums.0 { // SP NEAR changed
                    let delta = old_nums.0 - nums.0;
                    self.live.long.credit = self.live.long.credit
                        .checked_sub(delta).expect(ERR_SUB);
                    self.blood.debit = self.blood.debit
                        .checked_add(delta).expect(ERR_ADD);
                }
                if old_nums.2 > nums.2 { // SP QD changed
                    let delta = old_nums.2 - nums.2;
                    self.live.long.debit = self.live.long.debit
                        .checked_add(delta).expect(ERR_ADD);
                    self.blood.credit = self.blood.credit
                        .checked_add(delta).expect(ERR_ADD);
                }
                self.snatch(nums.3, nums.1, false);
                return (old_nums.0, 0, old_nums.2, 0);
            } else if cr < MIN_CR {
                (nums.1, nums.3) = self.shrink(nums.1, nums.3, false);
            }
        }
        return nums;
    }    

    pub(crate) fn shrink(&mut self, credit: Balance, debit: Balance, short: bool) -> (Balance, Balance) {
        /* Shrinking is atomically selling an amount of collateral and 
           immediately using the exact output of that to reduce debt to
           get its CR up to min. How to calculate amount to be sold:
           CR = (coll - x) / (debt - x)
           CR * debt - CR * x = coll - x
           x(1 - CR) = coll - CR * debt
           x = (CR * debt - coll) * 10
       */
       let mut coll: Balance;
       let mut debt: Balance;
       if short {
           coll = credit;
           debt = ratio(self.get_price(), debit, KILL_CR);
       } else {
           coll = ratio(self.get_price(), credit, KILL_CR);
           debt = debit;
       }
       let CR_x_debt = ratio(MIN_CR, debt, KILL_CR);
       let mut delta: Balance = 10;
       delta = delta.checked_mul(
           CR_x_debt.checked_sub(coll).expect(ERR_SUB)
       ).expect(ERR_MUL);
       coll = coll.checked_sub(delta).expect(ERR_SUB);
       debt = debt.checked_sub(delta).expect(ERR_SUB);
       if short {
           self.redeem(delta);
           self.live.short.credit = self.live.short.credit
               .checked_sub(delta).expect(ERR_SUB);
           delta = ratio(KILL_CR, delta, self.get_price());
           self.live.short.debit = self.live.short.debit
               .checked_sub(delta).expect(ERR_SUB);
           return (
               coll,
               ratio(KILL_CR, debt, self.get_price()),
           );
       } else {
           self.live.long.debit = self.live.long.debit
               .checked_sub(delta).expect(ERR_SUB);
           delta = ratio(KILL_CR, delta, self.get_price());
           self.invert(delta);
           self.live.long.credit = self.live.long.credit
               .checked_sub(delta).expect(ERR_SUB);
           return (
               ratio(KILL_CR, coll, self.get_price()),
               debt
           );    
       }
   }

   pub(crate) fn long_save(&mut self, pledge: &Pledge, available: Balance) -> (Balance, Balance, Balance, Balance) {
       let mut near = pledge.near;
       let mut quid = pledge.quid;
       let mut credit = pledge.long.credit;
       let mut debit = pledge.long.debit;
       // attempt to rescue the Pledge by dipping into its SolvencyPool deposit (if any)
       // try NEAR deposit *first*, because long liquidation means NEAR is falling, so
       // we want to keep as much QD in the SolvencyPool as we can before touching it 
       /*  How much to increase collateral of long side of pledge, to get CR to 110
           CR = ((coll + x) * price) / debt
           CR * debt / price = coll + x
           x = CR * debt / price - coll
           ^ subtracting the same units
       */ 
       let mut delta = ratio(MIN_CR, debit, self.get_price())
           .checked_sub(credit).expect(ERR_SUB);
       
       let mut min = std::cmp::min(near, delta);
       near -= min;
       credit = credit
           .checked_add(min).expect(ERR_ADD);
       self.live.long.credit = self.live.long.credit
           .checked_add(min).expect(ERR_ADD);
       self.blood.debit = self.blood.debit
           .checked_sub(min).expect(ERR_SUB);

       if delta > min {
           /*  how much to decrease long side's debt
               of  pledge, to get its CR up to min
               CR = (coll * price) / (debt - x)
               debt - x = (coll * price) / CR
               x = debt - (coll * price) / CR
               ^ subtracting the same units
           */
           delta = debit.checked_sub( // find remaining delta using updated credit
               ratio(self.get_price(), credit, MIN_CR)
           ).expect(ERR_SUB);
           // first, try to claim liquid QD from user's FungibleToken balance
           min = std::cmp::min(available, delta);
           delta -= min;
           debit = debit
               .checked_sub(min).expect(ERR_SUB);
           // we only withdraw, but do not deposit because we are burning debt 
           self.token.internal_withdraw(&pledge.id, min);
           
           if delta > 0 {
               min = std::cmp::min(quid, delta);
               quid -= min;
               debit = debit
                   .checked_sub(min).expect(ERR_SUB);
               self.live.long.debit = self.live.long.debit
                   .checked_sub(min).expect(ERR_SUB);
               self.blood.credit = self.blood.credit
                   .checked_sub(min).expect(ERR_SUB);
           }
       }
       return (near, credit, quid, debit); // we did the best we could, 
       // but there is no guarantee that the CR is back up to MIN_CR
   }

   pub(crate) fn short_save(&mut self, pledge: &Pledge, available: Balance) -> (Balance, Balance, Balance, Balance) {
       let mut near = pledge.near;
       let mut quid = pledge.quid;
       let mut credit = pledge.short.credit;
       let mut debit = pledge.short.debit;
       
       // attempt to rescue the Pledge using its SolvencyPool deposit (if any exists)
       // try QD deposit *first*, because short liquidation means NEAR is rising, so
       // we want to keep as much NEAR in the SolvencyPool as we can before touching it
       let val_debt = ratio(self.get_price(), debit, ONE);
        // first, try to claim liquid QD from user's FungibleToken balance
       // if they have NEAR in the SP it should stay there b/c it's growing
       // as we know this is what put the short in jeopardy of liquidation
       let final_qd = ratio(MIN_CR, val_debt, KILL_CR);
       let mut delta = final_qd.checked_sub(credit).expect(ERR_SUB);
       // first, try to claim liquid QD from user's FungibleToken balance
       let mut min = std::cmp::min(available, delta);
       delta -= min;

       credit = credit.checked_add(min).expect(ERR_ADD);
       self.token.internal_withdraw(&pledge.id, min);
       self.token.internal_deposit(&env::current_account_id(), min);
       self.live.short.credit = self.live.short.credit
           .checked_add(min).expect(ERR_ADD);
       
       if delta > 0 {
           min = std::cmp::min(quid, delta);
           credit = credit.checked_add(min).expect(ERR_ADD);
           self.live.short.credit = self.live.short.credit
               .checked_add(min).expect(ERR_ADD);
           
           delta -= min;
           quid -= min;
           self.blood.credit -= min;

           if delta > 0 {
               /*  How much to decrease debt of long side of pledge, to get its CR up to min
                   CR = coll / (debt * price - x)
                   debt * price - x = coll / CR
                   x = debt * price - coll / CR
               */
               delta = val_debt.checked_sub(
                   ratio(ONE, credit, MIN_CR)
               ).expect(ERR_SUB);
               
               min = std::cmp::min(near, delta);
               near -= min;
               debit -= min;
               self.blood.debit -= min;
               self.live.short.debit = self.live.short.debit
                   .checked_sub(min).expect(ERR_SUB);
           }
       }
       return (quid, credit, near, debit);
   }

   /**
     * You have to bring it back. That's why every magic trick has
     * a third act, the hardest part, which we call "The Prestige"
    */
    pub(crate) fn snatch(&mut self, debt: Balance, collat: Balance, short: bool) {
        if short { // we are moving crypto debt and QD collateral from LivePool to DeadPool
            self.live.short.credit = self.live.short.credit
                .checked_sub(collat).expect(ERR_SUB);

            self.dead.short.credit = self.dead.short.credit
                .checked_add(collat).expect(ERR_ADD);

            self.live.short.debit = self.live.short.debit
                .checked_sub(debt).expect(ERR_SUB);
            
            let val_debt = ratio(self.get_price(), debt, ONE);
            
            let delta = val_debt - collat; 
            assert!(delta > 0, "Borrower was not supposed to be liquidated");
            
            let delta_debt = ratio(ONE, delta, self.get_price());
                
            let debt_minus_delta = debt - delta_debt;
    
            self.dead.short.debit = self.dead.short.debit
                .checked_add(debt_minus_delta).expect(ERR_ADD);
            
            self.gfund.short.debit = self.gfund.short.debit
                .checked_add(delta_debt).expect(ERR_ADD);
        } 
        else { // we are moving QD debt and crypto collateral
            self.live.long.credit = self.live.long.credit
                .checked_sub(collat).expect(ERR_SUB);

            self.dead.long.credit = self.dead.long.credit
                .checked_add(collat).expect(ERR_ADD);
            
            self.live.long.debit = self.live.long.debit
                .checked_sub(debt).expect(ERR_SUB);

            let val_coll = ratio(self.get_price(), collat, ONE);

            let delta = debt - val_coll;
            assert!(delta > 0, "Borrower was not supposed to be liquidated");
            let debt_minus_delta = debt - delta;

            self.dead.long.debit = self.dead.long.debit
                .checked_add(debt_minus_delta).expect(ERR_ADD);
            
            self.gfund.long.debit = self.dead.long.debit
                .checked_add(delta).expect(ERR_ADD);
        }
    }
}