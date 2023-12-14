
var chai = require('chai')   
var assert = chai.assert 
const BN = require('bn.js') 

// const { NEAR, Gas, parse } = require('near-units') 

const nearAPI = require('near-api-js') 
const testUtils = require('./test-utils') 
const { contractAccount } = require('./near-utils')
const getConfig = require('./config') 

const { Account, functionCall, utils: { format: { parseNearAmount }} } = nearAPI 
const { connection, initContract, getAccount, getContract } = testUtils 
const { GAS, contractName } = getConfig() 

describe('Tests for ' + contractName, () => {
	let alice
	let bob
	let carl
	let david
	before(async () => {
		await initContract() 

		let state = (await new Account(connection, contractName)).state() 
		assert(state.code_hash !== '11111111111111111111111111111111', "bad") 

		alice = await getAccount()
		bob = await getAccount()
		carl = await getAccount()
		david = await getAccount()
	}) 
    it('borrow QD against NEAR', async () => {
		const contract = await getContract(alice)
		
		// var deposited = parseNearAmount('1.00')
		var borrowed = parseNearAmount('12')	
		var deposited = parseNearAmount('1.0')
		// // var borrowed = parseNearAmount('6.755')	

		try { // should fail
			var result = await alice.functionCall(contractName, 'borrow', {
				amount: borrowed, 
				short: false
			}, GAS, deposited) 
			
			// var result = await bob.functionCall(contractName, 'deposit', {
			// 	qd_amt: parseNearAmount('0.5'), // not staking QD...
			// 	//  NEAR with attached_deposit...not staking collat
			// 	live: false // it's a solvency deposit
			// }, GAS, 1) 
			// console.log('deposited')
			
			// console.log(deposited)
			// var result = await alice.functionCall(contractName, 'swap', {
			// 	amount: "0", 				
			// 	repay: false,
			// 	short: true
			// }, GAS, parseNearAmount('0.4')) 

		} catch (err) {
			assert.include(err.message, "Cannot do operation that would result in CR below min")
		}
		// try { // should fail
		// 	var result = await alice.functionCall(contractName, 'borrow', {
		// 		amount: borrowed, 
		// 		short: false
		// 	}, GAS, deposited) 
		// } catch (err) {
		// 	assert.include(err.message, "Cannot do operation that would result in CR below min")
		// }
		// let tmp = borrowed
		// borrowed = deposited
		// deposited = tmp

		// result = await alice.functionCall(contractName, 'borrow', {
		// 	amount: borrowed, 
		// 	short: false
		// }, GAS, deposited) 
		
		let balance = await contract.get_qd_balance({ account: alice.accountId}) 
		let pledge = await contract.get_pledge({ account: alice.accountId}) 
		let stats = await contract.get_pool_stats({}) 
		
		console.log('pledge', pledge)
		console.log('stats', stats)
		console.log('balance', balance)
		
		// assert.equal(stats.live_long_debit, borrowed)
		// assert.equal(stats.live_long_credit, deposited)
	})

	// it('deposit NEAR into SP so it can be loaned out', async () => {
	// 	const contract = await getContract(bob) 
	// 	let final = parseNearAmount('2.42')
	// 	var deposit = parseNearAmount('2.69')

	// 	var result = await bob.functionCall(contractName, 'deposit', {
	// 		amount: parseNearAmount('0'), // not staking QD, but NEAR with attached_deposit
	// 		live: false // not staking collateral, but solvency deposit
	// 	}, GAS, deposit) 

	// 	result = await bob.functionCall(contractName, 'renege', {
	// 		amount: parseNearAmount('0.27'), // 2.69 - 2.42
	// 		sp: true, // we are withdrawing from the SolvencyPool
	// 		qd: false, // we are withdrawing NEAR
	// 	}, GAS) 

	// 	const pledge = await contract.get_pledge({ account: bob.accountId}) 
	// 	assert.equal(pledge.near_sp, final)

	// 	const stats = await contract.get_pool_stats({}) 
	// 	assert.equal(stats.spool_debit, final)
	// })

	// it('borrow NEAR against QD', async () => {
	// 	const contract = await getContract(alice) 
	// 	let borrowed = parseNearAmount('3.0')
	// 	let available = parseNearAmount('2.42') // staked amount from last unit
	// 	let minimum = parseNearAmount('2.662')

	// 	let balanceBefore = await alice.getAccountBalance() 
	// 	console.log('balanceBefore', balanceBefore)
		
	// 	let result = await alice.functionCall(contractName, 'borrow', {
	// 		amount: borrowed, 
	// 		short: true
	// 	}, GAS, 1)
	// 	let pledge = await contract.get_pledge({ account: alice.accountId}) 
	// 	let stats = await contract.get_pool_stats({}) 

	// 	assert.equal(pledge.s_debit, available) 
	// 	assert.equal(pledge.s_credit, minimum)	
		
	// 	assert.equal(stats.live_short_debit, available)
	// 	assert.equal(stats.live_short_credit, minimum)

	// 	let balanceAfter = await alice.getAccountBalance()
	// 	let bb = new BN(balanceBefore.available)
	// 	let ba = new BN(balanceAfter.available)

	// 	let delta = ba.sub(bb).toString(10).substring(3)
	// 	assert.isAbove(parseFloat(delta), 2.416)
	// }) 
	// it('describe your test', async () => {
	// 	const contract = await getContract(alice) 
		


	// }) 

	// it('borrow short', async () => {
	// 	alice = await getAccount()

	// 	console.log('alice')
	// 	console.log(alice)
	// 	console.log('bob') 
	// 	console.log(bob) // TODO bob has the same accountId as Alice :/
		
	// 	const contract = await getContract(alice) 
	// 	// let result = await contract.borrow({ amount: parseNearAmount('0.123'), short: false }, GAS, parseNearAmount('1.0')) 
	// 	let amt = parseNearAmount('0.123')
		
	// 	let result = await alice.functionCall(contractName, 'borrow', {
	// 		amount: amt, 
	// 		short: false
	// 	}, GAS, parseNearAmount('1.0')) 
		
	// 	const pledge = await contract.get_pledge({ account: alice.accountId}) 
	// 	console.log(pledge)

	// 	const stats = await contract.get_pool_stats({}) 
	// 	console.log(stats)
	// 	let live_long_debit = stats.live_long_debit
	// 	console.log('live_long_debit', live_long_debit)
	// 	expect(live_long_debit).toEqual(amt) 

	// 	const pstats = await contract.get_stats({account: alice.accountId, short: false}) 
	// 	console.log(pstats)

	// 	let balanceBefore = await alice.getAccountBalance() 
	// 	console.log('balanceBefore', balanceBefore)

	// 	// expect(balance).toEqual(parseNearAmount('1')) 
	// }) 
}) 
