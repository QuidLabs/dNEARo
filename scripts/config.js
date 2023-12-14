
// Load environment variables
require("dotenv").config();

const near = require("near-api-js");

const contractName = 'dev-1650458016217-97126526854058';

// Configure the directory where NEAR credentials are going to be stored
// const credentialsPath = "./credentials";

// Configure the keyStore to be used with the NEAR Javascript API
// const UnencryptedFileSystemKeyStore = near.keyStores.UnencryptedFileSystemKeyStore;
// const keyStore = new UnencryptedFileSystemKeyStore(credentialsPath);

module.exports = function getConfig() {
	let config = {
		networkId:   process.env.NEAR_NETWORK,
        nodeUrl:     'https://rpc.testnet.near.org', //process.env.NEAR_NODE_URL,
        walletUrl:   `https://wallet.${process.env.NEAR_NETWORK}.near.org`,
        helperUrl:   `https://helper.${process.env.NEAR_NETWORK}.near.org`,
        explorerUrl: `https://explorer.${process.env.NEAR_NETWORK}.near.org`,
		contractName,
		contractMethods: {
			viewMethods: [
				"get_pledge",
				"get_qd_balance",
				"get_pledges",
				"get_pool_stats",
				"get_pledge_stats"
			  ], // our read function
			  changeMethods: [
				"deposit", // add param vote
				"borrow",
				"renege",
				"fold",
				"swap",
				"split",
				// not called by users
				"update",
				"clip",	
				"new"
			  ],
		},
	};
	if (!process.env.PROD) {
		config = {
			...config,
			GAS: '300000000000000',
			DEFAULT_NEW_ACCOUNT_AMOUNT: '5',
		};
	}
	else { // AKA prod
		config = {
			...config,
			networkId: 'mainnet',
			nodeUrl: 'https://rpc.mainnet.near.org',
			walletUrl: 'https://wallet.near.org',
			helperUrl: 'https://helper.mainnet.near.org',
			contractName: 'quid.near',
			// keyStore:    keyStore
		};
	}
	return config;
};
