//! Blockchain "backend": every call to the wallet or to a contract lives here.
//! The UI (components/, pages/) only calls these functions and never talks to the wallet itself.
//!
//! - Reads (`eth_call`) go straight to an RPC node over HTTP, so pages work before a wallet connects.
//! - Writes (`eth_sendTransaction`) go through the connected wallet, which signs them.

use std::{cell::RefCell, collections::HashMap, rc::Rc};

use alloy_primitives::{Address, B256, Bytes, U256, hex, keccak256};
use alloy_sol_types::{SolCall, sol};
use futures::future::{Either, select};
use gloo_net::http::Request;
use gloo_timers::future::TimeoutFuture;
use js_sys::{Array, Function, JSON, Object, Promise, Reflect};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

pub const EVALUATOR: &str = "0xe446502701fA5808DB07D817B00B660fB5EA6bFB";
pub const FAKE_BAYC: &str = "0x262fc337408995376b35f950e98b7fB60CF691f5";
pub const FAKE_NEFTURIANS: &str = "0x4F4f92733eEEe8405764EaA6D2B7304B59642469";
pub const FAKE_MEEBITS_CLAIMER: &str = "0xA45C08F929dfEf9a2B3E9eB3A5f9728F594f215a";
const MULTICALL3: &str = "0xcA11bde05977b3631167028862bE2a173976CA11"; // same address on every chain
const ENS_REGISTRY: &str = "0x00000000000C2E074eC69A0dFb2997BA6C7d2e1e"; // mainnet

const SEPOLIA: &str = "0xaa36a7"; // 11155111

// Set in .env (read at build time by build.rs). Everything compiled in is public in the browser.
const SEPOLIA_RPC: &str = match option_env!("SEPOLIA_RPC_URL") {
    Some(url) => url,
    None => "https://ethereum-sepolia-rpc.publicnode.com",
};
const MAINNET_RPC: &str = match option_env!("MAINNET_RPC_URL") {
    Some(url) => url,
    None => "https://ethereum-rpc.publicnode.com",
};
const WALLETCONNECT_PROJECT_ID: Option<&str> = option_env!("WALLETCONNECT_PROJECT_ID");

/// Tried in order, each with a timeout.
const IPFS_GATEWAYS: [&str; 4] = [
    "https://gateway.pinata.cloud/ipfs/",
    "https://ipfs.io/ipfs/",
    "https://dweb.link/ipfs/",
    "https://w3s.link/ipfs/",
];
const GATEWAY_TIMEOUT_MS: u32 = 12_000;

sol! {
    interface IERC721 {
        function name() external view returns (string);
        function totalSupply() external view returns (uint256);
        function tokenURI(uint256 tokenId) external view returns (string);
        function balanceOf(address owner) external view returns (uint256);
        function tokenOfOwnerByIndex(address owner, uint256 index) external view returns (uint256);
    }
    interface IFakeBAYC {
        function claimAToken() external;
    }
    interface IFakeNefturians {
        function tokenPrice() external view returns (uint256);
        function buyAToken() external payable;
    }
    interface IFakeMeebitsClaimer {
        function tokensThatWereClaimed(uint256) external view returns (bool);
        function claimAToken(uint256 tokenId, bytes signature) external payable returns (bool);
    }
    interface IEvaluator {
        function submitExercise() external;
        function ex1_claimedFakeBAYC() external;
        function ex2_boughtFakeNefturians() external;
        function ex3_claimedFakeMeebits() external;
    }
    interface IMulticall3 {
        struct Call3 { address target; bool allowFailure; bytes callData; }
        struct Res { bool success; bytes returnData; }
        function aggregate3(Call3[] calls) external payable returns (Res[] returnData);
    }
    interface IEnsRegistry {
        function resolver(bytes32 node) external view returns (address);
    }
    interface IEnsResolver {
        function name(bytes32 node) external view returns (string);
        function addr(bytes32 node) external view returns (address);
    }
}

// ---------------------------------------------------------------------------
// Data types the UI displays
// ---------------------------------------------------------------------------

/// JSON returned by a FakeBAYC `tokenURI` (no name/description fields).
#[derive(Clone, Debug, Deserialize)]
pub struct TokenMetadata {
    pub image: String,
    #[serde(default)]
    pub attributes: Vec<Attribute>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Attribute {
    pub trait_type: String,
    pub value: Value, // can be a string or a number in the JSON
}

#[derive(Deserialize)]
struct SignatureEntry {
    #[serde(rename = "tokenNumber")]
    token_number: u64,
    signature: String,
}

thread_local! {
    /// Wallet used for requests: WalletConnect once connected, else window.ethereum.
    static PROVIDER: RefCell<Option<JsValue>> = const { RefCell::new(None) };
    static LISTENERS: RefCell<Option<Listeners>> = const { RefCell::new(None) };
    static METADATA_CACHE: RefCell<HashMap<u64, TokenMetadata>> = RefCell::new(HashMap::new());
    static SIGNATURES: RefCell<Option<Rc<HashMap<u64, String>>>> = const { RefCell::new(None) };
}

#[derive(Clone)]
struct Listeners {
    accounts: Rc<dyn Fn(Option<String>)>,
    chain: Rc<dyn Fn(u64)>,
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn address(s: &str) -> Address {
    s.parse().expect("valid address constant")
}

fn parse_hex_u64(hex: &str) -> Result<u64, String> {
    u64::from_str_radix(hex.trim_start_matches("0x"), 16).map_err(|e| format!("bad hex number {hex}: {e}"))
}

fn to_u64(v: U256) -> Result<u64, String> {
    if v.bit_len() > 64 { Err(format!("number too large: {v}")) } else { Ok(v.to::<u64>()) }
}

fn to_u128(v: U256) -> Result<u128, String> {
    if v.bit_len() > 128 { Err(format!("number too large: {v}")) } else { Ok(v.to::<u128>()) }
}

fn is_revert(message: &str) -> bool {
    message.to_lowercase().contains("revert")
}

/// ENS namehash (EIP-137).
fn namehash(name: &str) -> B256 {
    name.rsplit('.').fold(B256::ZERO, |node, label| {
        keccak256([node.as_slice(), keccak256(label.as_bytes()).as_slice()].concat())
    })
}

// ---------------------------------------------------------------------------
// Reads over HTTP JSON-RPC
// ---------------------------------------------------------------------------

async fn rpc(url: &str, method: &str, params: Value) -> Result<Value, String> {
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    let response: Value = Request::post(url)
        .json(&body)
        .map_err(|e| e.to_string())?
        .send()
        .await
        .map_err(|e| format!("RPC unreachable: {e}"))?
        .json()
        .await
        .map_err(|e| format!("bad RPC response: {e}"))?;
    if let Some(error) = response.get("error") {
        return Err(error["message"].as_str().unwrap_or("RPC error").to_string());
    }
    Ok(response["result"].clone())
}

/// `eth_call` a contract view function and decode its return value.
async fn call<C: SolCall>(url: &str, to: Address, call: C) -> Result<C::Return, String> {
    let data = hex::encode_prefixed(call.abi_encode());
    let result = rpc(url, "eth_call", json!([{ "to": to.to_string(), "data": data }, "latest"])).await?;
    let bytes = hex::decode(result.as_str().ok_or("eth_call returned no data")?).map_err(|e| e.to_string())?;
    C::abi_decode_returns(&bytes).map_err(|e| format!("cannot decode {}: {e}", C::SIGNATURE))
}

/// Many view calls in one `eth_call` through Multicall3. None = that call reverted.
async fn multicall(calls: Vec<(Address, Vec<u8>)>) -> Result<Vec<Option<Bytes>>, String> {
    let mut out = Vec::with_capacity(calls.len());
    for chunk in calls.chunks(500) {
        let calls = chunk
            .iter()
            .map(|(target, data)| IMulticall3::Call3 {
                target: *target,
                allowFailure: true,
                callData: data.clone().into(),
            })
            .collect();
        let results = call(SEPOLIA_RPC, address(MULTICALL3), IMulticall3::aggregate3Call { calls }).await?;
        out.extend(results.into_iter().map(|r| r.success.then_some(r.returnData)));
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Wallet (EIP-1193 provider)
// ---------------------------------------------------------------------------

fn injected() -> Option<JsValue> {
    let ethereum = Reflect::get(&js_sys::global(), &"ethereum".into()).ok()?;
    (!ethereum.is_undefined() && !ethereum.is_null()).then_some(ethereum)
}

fn provider() -> Result<JsValue, String> {
    PROVIDER
        .with_borrow(|p| p.clone())
        .or_else(injected)
        .ok_or_else(|| "No wallet found. Install MetaMask or use WalletConnect.".into())
}

fn is_active(source: &JsValue) -> bool {
    provider().is_ok_and(|p| Object::is(&p, source))
}

/// Call `method` on a JS object and await the result if it is a Promise.
async fn js_call(target: &JsValue, method: &str, args: &[JsValue]) -> Result<JsValue, JsValue> {
    let function: Function = Reflect::get(target, &method.into())?.dyn_into()?;
    let result = function.apply(target, &args.iter().collect::<Array>())?;
    JsFuture::from(Promise::resolve(&result)).await
}

/// Raw EIP-1193 request; Err keeps the wallet's error object (with `code`).
async fn request_raw(method: &str, params: Value) -> Result<JsValue, JsValue> {
    let provider = provider().map_err(|e| JsValue::from(js_sys::Error::new(&e)))?;
    let args = JSON::parse(&json!({ "method": method, "params": params }).to_string())?;
    js_call(&provider, "request", &[args]).await
}

fn error_field(e: &JsValue, field: &str) -> JsValue {
    Reflect::get(e, &field.into()).unwrap_or(JsValue::UNDEFINED)
}

/// Best human message in a wallet error (`data.message` holds the revert reason in MetaMask).
fn error_message(e: JsValue) -> String {
    error_field(&error_field(&e, "data"), "message")
        .as_string()
        .or_else(|| error_field(&e, "message").as_string())
        .or_else(|| e.as_string())
        .unwrap_or_else(|| format!("{e:?}"))
}

/// EIP-1193 request; maps wallet errors (e.g. user rejected) to their message.
async fn request(method: &str, params: Value) -> Result<JsValue, String> {
    request_raw(method, params).await.map_err(error_message)
}

fn first_account(accounts: &JsValue) -> Option<String> {
    Array::from(accounts).get(0).as_string()
}

/// Forward wallet events to the stored listeners, only while `provider` is the active wallet.
fn attach_listeners(provider: &JsValue) {
    let Some(listeners) = LISTENERS.with_borrow(|l| l.clone()) else { return };

    let (source, accounts_cb) = (provider.clone(), listeners.accounts);
    let on_accounts = Closure::<dyn Fn(JsValue)>::new(move |accounts: JsValue| {
        if is_active(&source) {
            accounts_cb(first_account(&accounts));
        }
    });
    let (source, chain_cb) = (provider.clone(), listeners.chain);
    let on_chain = Closure::<dyn Fn(JsValue)>::new(move |id: JsValue| {
        if let (true, Some(Ok(id))) = (is_active(&source), id.as_string().map(|id| parse_hex_u64(&id))) {
            chain_cb(id);
        }
    });

    if let Ok(on) = Reflect::get(provider, &"on".into()).and_then(|f| f.dyn_into::<Function>()) {
        let _ = on.call2(provider, &"accountsChanged".into(), on_accounts.as_ref());
        let _ = on.call2(provider, &"chainChanged".into(), on_chain.as_ref());
    }
    // The wallet keeps calling these for the whole page lifetime.
    on_accounts.forget();
    on_chain.forget();
}

/// Call `on_accounts` / `on_chain` whenever the user changes account or network in the wallet.
pub fn subscribe(on_accounts: impl Fn(Option<String>) + 'static, on_chain: impl Fn(u64) + 'static) {
    LISTENERS.set(Some(Listeners {
        accounts: Rc::new(on_accounts),
        chain: Rc::new(on_chain),
    }));
    if let Some(ethereum) = injected() {
        attach_listeners(&ethereum);
    }
}

/// Account already authorized for this site (no popup), e.g. after a page reload.
pub async fn current_account() -> Result<Option<String>, String> {
    if provider().is_err() {
        return Ok(None);
    }
    Ok(first_account(&request("eth_accounts", json!([])).await?))
}

/// Connect the browser wallet. Returns the first address.
pub async fn connect_injected() -> Result<String, String> {
    let ethereum = injected().ok_or("No browser wallet found. Install MetaMask or use WalletConnect.")?;
    PROVIDER.set(Some(ethereum));
    let accounts = request("eth_requestAccounts", json!([])).await?;
    first_account(&accounts).ok_or_else(|| "The wallet returned no account".into())
}

/// Connect a mobile wallet with a QR code (WalletConnect v2). Returns the first address.
pub async fn connect_walletconnect() -> Result<String, String> {
    let project_id = WALLETCONNECT_PROJECT_ID
        .ok_or("WalletConnect is not configured: set WALLETCONNECT_PROJECT_ID in .env and rebuild.")?;
    let wc = Reflect::get(&js_sys::global(), &"WcProvider".into()).unwrap_or(JsValue::UNDEFINED);
    if wc.is_undefined() {
        return Err("WalletConnect bundle not loaded (public/wc.js). Run `npm run build` in wc/.".into());
    }
    let options = JSON::parse(
        &json!({
            "projectId": project_id,
            "chains": [11_155_111],
            "showQrModal": true,
            "rpcMap": { "11155111": SEPOLIA_RPC },
        })
        .to_string(),
    )
    .map_err(error_message)?;

    let provider = js_call(&wc, "init", &[options]).await.map_err(error_message)?;
    let accounts = js_call(&provider, "enable", &[]).await.map_err(error_message)?;
    PROVIDER.set(Some(provider.clone()));
    attach_listeners(&provider);
    first_account(&accounts).ok_or_else(|| "The wallet returned no account".into())
}

/// Current chain id of the wallet.
pub async fn chain_id() -> Result<u64, String> {
    let id = request("eth_chainId", json!([])).await?;
    parse_hex_u64(&id.as_string().ok_or("eth_chainId returned no string")?)
}

/// Switch the wallet to Sepolia; add the chain first if the wallet doesn't know it (error 4902).
pub async fn switch_to_sepolia() -> Result<(), String> {
    match request_raw("wallet_switchEthereumChain", json!([{ "chainId": SEPOLIA }])).await {
        Ok(_) => Ok(()),
        Err(e) if error_field(&e, "code").as_f64() == Some(4902.0) => request(
            "wallet_addEthereumChain",
            json!([{
                "chainId": SEPOLIA,
                "chainName": "Sepolia",
                "nativeCurrency": { "name": "Sepolia Ether", "symbol": "ETH", "decimals": 18 },
                "rpcUrls": ["https://ethereum-sepolia-rpc.publicnode.com"],
                "blockExplorerUrls": ["https://sepolia.etherscan.io"],
            }]),
        )
        .await
        .map(|_| ()),
        Err(e) => Err(error_message(e)),
    }
}

/// Primary ENS name of `address` (mainnet), or None. The name must resolve back to the address.
pub async fn ens_name(address_hex: &str) -> Result<Option<String>, String> {
    let owner: Address = address_hex.parse().map_err(|_| format!("invalid address {address_hex}"))?;
    let registry = address(ENS_REGISTRY);

    let node = namehash(&format!("{}.addr.reverse", hex::encode(owner)));
    let resolver = call(MAINNET_RPC, registry, IEnsRegistry::resolverCall { node }).await?;
    if resolver.is_zero() {
        return Ok(None);
    }
    let Ok(name) = call(MAINNET_RPC, resolver, IEnsResolver::nameCall { node }).await else {
        return Ok(None);
    };
    if name.is_empty() {
        return Ok(None);
    }

    // Forward check: anyone can set any reverse name, only trust it if it points back.
    let node = namehash(&name);
    let resolver = call(MAINNET_RPC, registry, IEnsRegistry::resolverCall { node }).await?;
    if resolver.is_zero() {
        return Ok(None);
    }
    let resolved = call(MAINNET_RPC, resolver, IEnsResolver::addrCall { node }).await;
    Ok((resolved == Ok(owner)).then_some(name))
}

// ---------------------------------------------------------------------------
// Transactions
// ---------------------------------------------------------------------------

/// Connected account, with the wallet switched to Sepolia.
async fn ready_account() -> Result<String, String> {
    let accounts = request("eth_requestAccounts", json!([])).await?;
    let from = first_account(&accounts).ok_or("The wallet returned no account")?;
    if chain_id().await? != 11_155_111 {
        switch_to_sepolia().await?;
    }
    Ok(from)
}

/// Simulate, then send a transaction through the wallet. Returns the tx hash.
async fn send<C: SolCall>(to: &str, call: C, value: Option<U256>) -> Result<String, String> {
    let from = ready_account().await?;
    let mut tx = json!({ "from": from, "to": to, "data": hex::encode_prefixed(call.abi_encode()) });
    if let Some(value) = value {
        tx["value"] = json!(format!("{value:#x}"));
    }
    // Dry run first: a revert reason from the node is clearer than the wallet's generic error.
    if let Err(e) = rpc(SEPOLIA_RPC, "eth_call", json!([tx, "latest"])).await
        && is_revert(&e)
    {
        return Err(e);
    }
    request("eth_sendTransaction", json!([tx]))
        .await?
        .as_string()
        .ok_or_else(|| "The wallet returned no tx hash".into())
}

/// Latest block number of the connected chain.
pub async fn block_number() -> Result<u64, String> {
    let n = request("eth_blockNumber", json!([])).await?;
    parse_hex_u64(&n.as_string().ok_or("eth_blockNumber returned no string")?)
}

/// Wait until `tx_hash` is mined. Ok(true) = success, Ok(false) = reverted.
pub async fn wait_for_receipt(tx_hash: &str) -> Result<bool, String> {
    for _ in 0..150 {
        let receipt = rpc(SEPOLIA_RPC, "eth_getTransactionReceipt", json!([tx_hash])).await?;
        if !receipt.is_null() {
            return Ok(receipt["status"] == "0x1");
        }
        TimeoutFuture::new(2_000).await;
    }
    Err("Still pending after 5 minutes. Check Etherscan.".into())
}

// ---------------------------------------------------------------------------
// Evaluator: every write returns the tx hash
// ---------------------------------------------------------------------------

pub async fn submit_exercise() -> Result<String, String> {
    send(EVALUATOR, IEvaluator::submitExerciseCall {}, None).await
}

pub async fn ex1_claimed_fake_bayc() -> Result<String, String> {
    send(EVALUATOR, IEvaluator::ex1_claimedFakeBAYCCall {}, None).await
}

pub async fn ex2_bought_fake_nefturians() -> Result<String, String> {
    send(EVALUATOR, IEvaluator::ex2_boughtFakeNefturiansCall {}, None).await
}

pub async fn ex3_claimed_fake_meebits() -> Result<String, String> {
    send(EVALUATOR, IEvaluator::ex3_claimedFakeMeebitsCall {}, None).await
}

// ---------------------------------------------------------------------------
// ERC721 helpers shared by the collections
// ---------------------------------------------------------------------------

/// Every token id owned by `owner` (contracts are ERC721Enumerable).
async fn tokens_of(contract: &str, owner: &str) -> Result<Vec<u64>, String> {
    let contract = address(contract);
    let owner: Address = owner.parse().map_err(|_| format!("invalid address {owner}"))?;
    let balance = to_u64(call(SEPOLIA_RPC, contract, IERC721::balanceOfCall { owner }).await?)?;
    let calls = (0..balance)
        .map(|i| {
            let index = U256::from(i);
            (contract, IERC721::tokenOfOwnerByIndexCall { owner, index }.abi_encode())
        })
        .collect();
    multicall(calls)
        .await?
        .into_iter()
        .map(|data| {
            let data = data.ok_or("tokenOfOwnerByIndex reverted")?;
            to_u64(IERC721::tokenOfOwnerByIndexCall::abi_decode_returns(&data).map_err(|e| e.to_string())?)
        })
        .collect()
}

/// `ipfs://CID/x` or `https://any-gateway/ipfs/CID/x` -> `CID/x`
fn ipfs_path(uri: &str) -> Option<&str> {
    uri.strip_prefix("ipfs://")
        .map(|p| p.trim_start_matches("ipfs/"))
        .or_else(|| uri.split_once("/ipfs/").map(|(_, p)| p))
}

fn http_url(uri: &str) -> String {
    match ipfs_path(uri) {
        Some(path) => format!("{}{path}", IPFS_GATEWAYS[0]),
        None => uri.to_string(),
    }
}

/// Fetch JSON, trying each IPFS gateway with a timeout.
async fn fetch_json<T: DeserializeOwned>(uri: &str) -> Result<T, String> {
    let urls: Vec<String> = match ipfs_path(uri) {
        Some(path) => IPFS_GATEWAYS.iter().map(|g| format!("{g}{path}")).collect(),
        None => vec![uri.to_string()],
    };
    let mut last_error = String::new();
    for url in urls {
        let fetch = async {
            let response = Request::get(&url).send().await.map_err(|e| e.to_string())?;
            if !response.ok() {
                return Err(format!("HTTP {}", response.status()));
            }
            response.json::<T>().await.map_err(|e| e.to_string())
        };
        match select(Box::pin(fetch), TimeoutFuture::new(GATEWAY_TIMEOUT_MS)).await {
            Either::Left((Ok(value), _)) => return Ok(value),
            Either::Left((Err(e), _)) => last_error = format!("{url}: {e}"),
            Either::Right(_) => last_error = format!("{url}: timed out"),
        }
    }
    Err(format!("Could not load metadata ({last_error})"))
}

// ---------------------------------------------------------------------------
// FakeBAYC
// ---------------------------------------------------------------------------

pub async fn bayc_name() -> Result<String, String> {
    call(SEPOLIA_RPC, address(FAKE_BAYC), IERC721::nameCall {}).await
}

pub async fn bayc_total_supply() -> Result<u64, String> {
    to_u64(call(SEPOLIA_RPC, address(FAKE_BAYC), IERC721::totalSupplyCall {}).await?)
}

pub async fn bayc_claim() -> Result<String, String> {
    send(FAKE_BAYC, IFakeBAYC::claimATokenCall {}, None).await
}

/// Metadata of a token (cached). Ok(None) = token does not exist.
pub async fn bayc_metadata(token_id: u64) -> Result<Option<TokenMetadata>, String> {
    if let Some(meta) = METADATA_CACHE.with_borrow(|c| c.get(&token_id).cloned()) {
        return Ok(Some(meta));
    }
    let uri = match call(
        SEPOLIA_RPC,
        address(FAKE_BAYC),
        IERC721::tokenURICall { tokenId: U256::from(token_id) },
    )
    .await
    {
        Ok(uri) => uri,
        Err(e) if is_revert(&e) => return Ok(None), // "ERC721: invalid token ID"
        Err(e) => return Err(e),
    };
    let mut meta: TokenMetadata = fetch_json(&uri).await?;
    meta.image = http_url(&meta.image);
    METADATA_CACHE.with_borrow_mut(|c| c.insert(token_id, meta.clone()));
    Ok(Some(meta))
}

pub async fn bayc_tokens_of(owner: &str) -> Result<Vec<u64>, String> {
    tokens_of(FAKE_BAYC, owner).await
}

// ---------------------------------------------------------------------------
// FakeNefturians
// ---------------------------------------------------------------------------

async fn nefturians_price() -> Result<U256, String> {
    call(SEPOLIA_RPC, address(FAKE_NEFTURIANS), IFakeNefturians::tokenPriceCall {}).await
}

pub async fn nefturians_price_wei() -> Result<u128, String> {
    to_u128(nefturians_price().await?)
}

/// The contract requires `msg.value > tokenPrice` (strictly), so pay price + 1 wei.
pub async fn nefturians_buy() -> Result<String, String> {
    let value = nefturians_price().await? + U256::from(1);
    send(FAKE_NEFTURIANS, IFakeNefturians::buyATokenCall {}, Some(value)).await
}

pub async fn nefturians_tokens_of(owner: &str) -> Result<Vec<u64>, String> {
    tokens_of(FAKE_NEFTURIANS, owner).await
}

// ---------------------------------------------------------------------------
// FakeMeebits
// ---------------------------------------------------------------------------

/// public/output-sig.json (from the TD repo's signatures/), loaded once.
async fn signatures() -> Result<Rc<HashMap<u64, String>>, String> {
    if let Some(sigs) = SIGNATURES.with_borrow(|s| s.clone()) {
        return Ok(sigs);
    }
    let entries: Vec<SignatureEntry> = fetch_json("/public/output-sig.json").await?;
    let sigs = Rc::new(entries.into_iter().map(|e| (e.token_number, e.signature)).collect());
    SIGNATURES.set(Some(Rc::clone(&sigs)));
    Ok(sigs)
}

/// Token ids that have a signature and are not claimed yet on FakeMeebitsClaimer.
pub async fn meebits_unclaimed_ids() -> Result<Vec<u64>, String> {
    let mut ids: Vec<u64> = signatures().await?.keys().copied().collect();
    ids.sort_unstable();
    let claimer = address(FAKE_MEEBITS_CLAIMER);
    let calls = ids
        .iter()
        .map(|&id| (claimer, IFakeMeebitsClaimer::tokensThatWereClaimedCall(U256::from(id)).abi_encode()))
        .collect();
    let claimed = multicall(calls).await?;
    Ok(ids
        .into_iter()
        .zip(claimed)
        .filter(|(_, data)| {
            data.as_ref()
                .and_then(|d| IFakeMeebitsClaimer::tokensThatWereClaimedCall::abi_decode_returns(d).ok())
                == Some(false)
        })
        .map(|(id, _)| id)
        .collect())
}

pub async fn meebits_signature(token_id: u64) -> Result<String, String> {
    signatures()
        .await?
        .get(&token_id)
        .cloned()
        .ok_or_else(|| format!("No signature for token #{token_id}"))
}

pub async fn meebits_claim(token_id: u64, signature: &str) -> Result<String, String> {
    let signature = hex::decode(signature).map_err(|e| format!("bad signature: {e}"))?;
    send(
        FAKE_MEEBITS_CLAIMER,
        IFakeMeebitsClaimer::claimATokenCall { tokenId: U256::from(token_id), signature: signature.into() },
        None,
    )
    .await
}
