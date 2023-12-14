
const BN = require('bn.js');
const nearAPI = require('near-api-js');
const { KeyPair, Account, Contract, utils: { format: { parseNearAmount } } } = nearAPI;
const { near, connection, keyStore, contract, contractAccount } = require('./near-utils')
const getConfig = require('./config');
const {
    networkId, contractName, contractMethods, DEFAULT_NEW_ACCOUNT_AMOUNT
} = getConfig();

/********************************
Internal Helpers
********************************/
async function createAccount(accountId, fundingAmount = DEFAULT_NEW_ACCOUNT_AMOUNT) {
	const contractAccount = new Account(connection, contractName);
	const newKeyPair = KeyPair.fromRandom('ed25519');
	await contractAccount.createAccount(accountId, newKeyPair.publicKey, new BN(parseNearAmount(fundingAmount)));
	keyStore.setKey(networkId, accountId, newKeyPair);
	return new nearAPI.Account(connection, accountId);
}

function generateUniqueSubAccount() {
	return `t${Date.now()}.${contractName}`;
}

/********************************
Exports
********************************/

const getAccountBalance = async (accountId) => (new nearAPI.Account(connection, accountId)).getAccountBalance();

const initAccount = async(accountId, secret) => {
	account = new nearAPI.Account(connection, accountId);
	const newKeyPair = KeyPair.fromString(secret);
	keyStore.setKey(networkId, accountId, newKeyPair);
	return account
}

async function initContract() {
	try {
		// let result = await contract.new({ owner_id: contractName }); // Same thing as below
		let result = await contractAccount.functionCall(
			contractName, "new",
			{ 
				"owner_id": contractName,
			}
		)
		// console.log(result)
	} catch (e) {
		if (!/Already initialized/.test(e.toString())) {
			throw e;
		}
	}
	return { contract, contractName };
}

const createOrInitAccount = async(accountId, secret) => {
	let account;
	try {
		account = await createAccount(accountId, DEFAULT_NEW_CONTRACT_AMOUNT, secret);
	} catch (e) {
		if (!/because it already exists/.test(e.toString())) {
			throw e;
		}
		account = new nearAPI.Account(connection, accountId);

		console.log(await getAccountBalance(accountId));

		const newKeyPair = KeyPair.fromString(secret);
		keyStore.setKey(networkId, accountId, newKeyPair);
	}
	return account;
};

async function getContract(account) {
	return new Contract(contractAccount, contractName, {
		...contractMethods,
		signer: account || undefined
	});
}

async function getAccount(accountId, fundingAmount = DEFAULT_NEW_ACCOUNT_AMOUNT) {
	accountId = accountId || generateUniqueSubAccount();
	const account = new nearAPI.Account(connection, accountId);
	try {
		await account.state();
		return account;
	} catch(e) {
		if (!/does not exist/.test(e.toString())) {
			throw e;
		}
	}
	return await createAccount(accountId, fundingAmount);
};

const getSignature = async (account) => {
	const { accountId } = account;
	const block = await account.connection.provider.block({ finality: 'final' });
	const blockNumber = block.header.height.toString();
	const signer = account.inMemorySigner || account.connection.signer;
	const signed = await signer.signMessage(Buffer.from(blockNumber), accountId, networkId);
	const blockNumberSignature = Buffer.from(signed.signature).toString('base64');
	return { blockNumber, blockNumberSignature };
};

const loadCredentials = (accountId) => {
	const credPath = `./neardev/${networkId}/${accountId}.json`;
	console.log(
		"Loading Credentials:\n",
		credPath
	);

	let credentials;
	try {
		credentials = JSON.parse(
			fs.readFileSync(
				credPath
			)
		);
	} catch(e) {
		console.warn('credentials not in /neardev');
		/// attempt to load backup creds from local machine
		credentials = JSON.parse(
			fs.readFileSync(
				`${process.env.HOME}/.near-credentials/${networkId}/${accountId}.json`
			)
		);
	}

	return credentials
}


module.exports = { 
    near,
    connection,
    keyStore,
    getContract,
	getSignature,
	loadCredentials,
    contract,
    contractName,
    contractAccount,
    initContract, getAccount,
	initAccount, createOrInitAccount
};
