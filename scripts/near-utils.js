
require("dotenv").config();
const fs = require('fs');
const nearAPI = require('near-api-js');
const getConfig = require('./config');
const { nodeUrl, networkId, contractName, contractMethods } = getConfig();
const {
    keyStores: { InMemoryKeyStore },
    Near, Account, Contract, KeyPair,
    utils: {
        format: {
            parseNearAmount
        }
    }
} = nearAPI;

// const credentials = JSON.parse(fs.readFileSync(process.env.HOME + '/.near-credentials/testnet/' + contractName + '.json'));
// const keyStore = {
//     keys: {},
// 	getKey(networkId, accountId) {
// 		const value = this.keys[`${accountId}:${networkId}`];
//         if (!value) {
//             return null;
//         }
//         return KeyPair.fromString(value);
//     },
//     setKey(networkId, accountId, keyPair) {
//         this.keys[`${accountId}:${networkId}`] = keyPair.toString();
//     }
// };
// keyStore.setKey(networkId, contractName, KeyPair.fromString(credentials.private_key))

const credPath = `./neardev/${networkId}/${contractName}.json`
console.log(
	"Loading Credentials:\n",
	credPath
);

let credentials
try {
	credentials = JSON.parse(
		fs.readFileSync(
			credPath
		)
	);
} catch(e) {
	console.warn(e)
	/// attempt to load backup creds from local machine
	credentials = JSON.parse(
		fs.readFileSync(
			`${process.env.HOME}/.near-credentials/${networkId}/${contractName}.json`
		)
	);
}
const keyStore = new InMemoryKeyStore();
keyStore.setKey(
	networkId,
	contractName,
	KeyPair.fromString(credentials.private_key)
);

const near = new Near({
	networkId, nodeUrl,
	deps: { keyStore },
});
const { connection } = near
const contractAccount = new Account(connection, contractName);
const contract = new Contract(contractAccount, contractName, contractMethods);

module.exports = {
    near,
    keyStore,
    connection,
    contract,
    contractName,
    contractAccount,
    contractMethods
}
