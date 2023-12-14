// Load environment variables
require("dotenv").config();

// Load Near Javascript API components
const near = require("near-api-js");
const { sha256 } = require("js-sha256");
const fs = require("fs");

// Configure the directory where NEAR credentials are going to be stored
const credentialsPath = "./credentials";

// Configure the keyStore to be used with the NEAR Javascript API
const UnencryptedFileSystemKeyStore = near.keyStores.UnencryptedFileSystemKeyStore;
const keyStore = new UnencryptedFileSystemKeyStore(credentialsPath);

// Setup default client options
const options = {
  networkId:   process.env.NEAR_NETWORK,
  nodeUrl:     process.env.NEAR_NODE_URL,
  walletUrl:   `https://wallet.${process.env.NEAR_NETWORK}.near.org`,
  helperUrl:   `https://helper.${process.env.NEAR_NETWORK}.near.org`,
  explorerUrl: `https://explorer.${process.env.NEAR_NETWORK}.near.org`,
  accountId:   process.env.NEAR_ACCOUNT, 
  keyStore:    keyStore
}

// Formatter helper for Near amounts
function formatAmount(amount) {
  return BigInt(near.utils.format.parseNearAmount(amount.toString()));
};

async function createAccount() {
  let keyPair;

  // Configure the client with options and our local key store
  const client = await near.connect(options);

  // Configure the key pair file location
  const keyRootPath = client.connection.signer.keyStore.keyDir;
  const keyFilePath = `${keyRootPath}/${options.networkId}/${options.accountId}.json`;

  // Check if the key pair exists, and create a new one if it does not
  if (!fs.existsSync(keyFilePath)) {
    console.log("Generating a new key pair")
    keyPair = near.KeyPair.fromRandom("ed25519");
  } else {
    let content = JSON.parse(fs.readFileSync(keyFilePath).toString());
    keyPair = near.KeyPair.fromString(content.private_key);

    console.log(`Key pair for account ${options.accountId} already exists, skipping creation`);
  }

  // Create a key pair in credentials directory
  await client.connection.signer.keyStore.setKey(options.networkId, options.accountId, keyPair);

  // Determine if account already exists
  try {
    await client.account(options.accountId)
    //return console.log(`Sorry, account '${options.accountId}' already exists.`);
  }
  catch (e) {
    if (!e.message.includes("does not exist while viewing")) {
      throw e;
    }
  }

  // Generate a public key for account creation step
  const publicKey = keyPair.getPublicKey()

  // Create the account
  try {
    const response = await client.createAccount(options.accountId, publicKey);
    console.log(`Account ${response.accountId} for network "${options.networkId}" was created.`);
    console.log("----------------------------------------------------------------");
    console.log("OPEN LINK BELOW to see account in NEAR Explorer!");
    console.log(`${options.explorerUrl}/accounts/${response.accountId}`);
    console.log("----------------------------------------------------------------");
  }
  catch(error) {
    console.log("ERROR:", error);
  }
}

async function getInfo() {
    // Configure the client with options
    const client = await near.connect(options);
    const provider = client.connection.provider;
    console.log("Client config:", client.config);

    // Let's get our own account status
    const account = await client.account(options.accountId);
    console.log("account state:", await account.state());
      
    // Get current node status
    const status = await provider.status();
    console.log("Status:", status);

    // Get the latest block
    let block = await provider.block({ finality: "final" });
    console.log("current block:", block);

    // Get the block by number
    block = await provider.block({ blockId: status.sync_info.latest_block_height });
    console.log("block by height:", block);

    // Get current gas price from the block header
    const anotherBlock = await provider.sendJsonRpc("block", { finality: "final" });
    console.log("gas price from header:", anotherBlock.header.gas_price);

    // Let's get the current validator set for the network
    const validators = await provider.validators(block.header.epoch_id);
    console.log("network validators:", validators);
}

async function makeTransfer() {
  const txSender = options.accountId;
  const txReceiver = "quid.testnet";
  const txAmount = formatAmount(1);

  const client = await near.connect(options);
  const account = await client.account(txSender);
  const provider = client.connection.provider;
  // Create a simple money transfer using helper method
  console.log(`Sending money to ${txReceiver}`);
  /*
  try {
    const result = await account.sendMoney(txReceiver, txAmount);

    console.log("Creation result:", result.transaction);
    console.log("----------------------------------------------------------------");
    console.log("OPEN LINK BELOW to see transaction in NEAR Explorer!");
    console.log(`${options.explorerUrl}/transactions/${result.transaction.hash}`);
    console.log("----------------------------------------------------------------");

    setTimeout(async function() {
      console.log("Checking transaction status:", result.transaction.hash);

      const status = await provider.sendJsonRpc("tx", [result.transaction.hash, options.accountId]);
      console.log("Transaction status:", status);
    }, 5000);
  }
  catch(error) {
    console.log("ERROR:", error);
  }
  */
  const keyRootPath = client.connection.signer.keyStore.keyDir;
  const keyFilePath = `${keyRootPath}/${options.networkId}/${options.accountId}.json`;
  // Load key pair from the file
  const content = JSON.parse(fs.readFileSync(keyFilePath).toString());
  const keyPair = near.KeyPair.fromString(content.private_key);

  // Get the sender public key
  const publicKey = keyPair.getPublicKey();
  console.log("Sender public key:", publicKey.toString())

  // Get the public key information from the node
  const accessKey = await provider.query(
    `access_key/${txSender}/${publicKey.toString()}`, ""
  );
  console.log("Sender access key:", accessKey);

  // Check to make sure provided key is a full access key
  if (accessKey.permission !== "FullAccess") {
    return console.log(`Account [${txSender}] does not have permission to send tokens using key: [${publicKey}]`);
  };

  // Each transaction requires a unique number or nonce
  // This is created by taking the current nonce and incrementing it
  const nonce = ++accessKey.nonce;
  console.log("Calculated nonce:", nonce);

   // Construct actions that will be passed to the createTransaction method below
   const actions = [near.transactions.transfer(txAmount)];

   // Convert a recent block hash into an array of bytes.
   // This is required to prove the tx was recently constructed (within 24hrs)
   const recentBlockHash = near.utils.serialize.base_decode(accessKey.block_hash);
 
   // Create a new transaction object
   const transaction = near.transactions.createTransaction(
     txSender,
     publicKey,
     txReceiver,
     nonce,
     actions,
     recentBlockHash
   );
 
   // Before we can sign the transaction we must perform three steps
   // 1) Serialize the transaction in Borsh
   const serializedTx = near.utils.serialize.serialize(
     near.transactions.SCHEMA,
     transaction
   );
 
   // 2) Hash the serialized transaction using sha256
   const serializedTxHash = new Uint8Array(sha256.array(serializedTx));
 
   // 3) Create a signature using the hashed transaction
   const signature = keyPair.sign(serializedTxHash);
 
   // Sign the transaction
   const signedTransaction = new near.transactions.SignedTransaction({
     transaction,
     signature: new near.transactions.Signature({
       keyType: transaction.publicKey.keyType,
       data: signature.signature
     })
   });
 
   // Send the transaction
   try {
     const result = await provider.sendTransaction(signedTransaction);
 
     console.log("Creation result:", result.transaction);
     console.log("----------------------------------------------------------------");
     console.log("OPEN LINK BELOW to see transaction in NEAR Explorer!");
     console.log(`${options.explorerUrl}/transactions/${result.transaction.hash}`);
     console.log("----------------------------------------------------------------");
 
     setTimeout(async function() {
       console.log("Checking transaction status:", result.transaction.hash);
 
       const status = await provider.sendJsonRpc("tx", [result.transaction.hash, options.accountId]);
       console.log("Transaction status:", status);
     }, 5000);
   }
   catch(error) {
     console.log("ERROR:", error);
   }
 
}

async function test() {
   // Configure the client with options and our local key store
   const client = await near.connect(options);
   const account = await client.account(options.accountId);
 
   // We'are using the same contract name, feel free to create a different one.
   const contractName = options.accountId;
 
   // Construct a new contract object, we'll be using it to perform calls
   const contract = new near.Contract(account, contractName, {
      viewMethods: [
        "get_pledge",
        "get_num_pledges",
        "get_pledges",
        "get_top"
      ], // our read function
      changeMethods: [
        "new",
        "sync",
        "register",
        "re_pledge",
        "clip",
        "renege",
        "borrow",
        "stake",
        "redeem",
      ], // our write function
      sender: options.accountId,   // account used to sign contract call transactions
   });
   
   // Initialize the contract
   
   try {
     result = await account.functionCall(
      contractName, "new",
      { 
        "owner_id": contractName,
        //"total_supply": "10" 
      }
     );
     console.log(result);
   } catch(e) {
     console.log(e)
   }
    
    // result = await account.functionCall(
    //   contractName, "borrow",
    //   { "amount": "8" }, 0, 10
    // );
    // console.log(result);
    result = await account.functionCall(
      contractName, "sync", {}
    );
    console.log(result);
    /*
    console.log("==== Get_NUM_Pledges ====");
    result = await contract.get_num_pledges({
      'account': contractName
    });
    console.log("Result:", result);
    */
    // console.log("==== Get_Pledge ====");
    // result = await contract.get_pledge({
    //   'account': contractName
    // });
    // console.log("Result:", result);
    
    // result = await account.functionCall(
    //   contractName, "renege",
    //   { "amount": "2", "ins": false }
    // );
    // console.log(result);
    
    // console.log("==== Get_Pledge ====");
    // result = await contract.get_pledge({
    //   'account': contractName
    // });
    // console.log("Result:", result);
}
  
//main();
test();