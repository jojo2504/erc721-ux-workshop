//! Reusable UI pieces: header, wallet bar, address label, transaction button.

use std::future::Future;

use leptos::{prelude::*, task::spawn_local};
use leptos_router::{
    components::A,
    hooks::{use_location, use_navigate},
};

use crate::{
    eth,
    wallet::{SEPOLIA_ID, Wallet, short_address},
};

#[component]
pub fn Header() -> impl IntoView {
    view! {
        <header>
            <nav>
                <A href="/">"Home"</A>
                <A href="/chain-info">"Chain info"</A>
                <A href="/fakeBayc">"FakeBAYC"</A>
                <A href="/fakeNefturians">"FakeNefturians"</A>
                <A href="/fakeMeebits">"FakeMeebits"</A>
            </nav>
            <WalletBar/>
            <ChainGuard/>
        </header>
    }
}

/// Redirects to /wrong-network whenever the connected chain is not Sepolia.
#[component]
fn ChainGuard() -> impl IntoView {
    let wallet = Wallet::get();
    let navigate = use_navigate();
    let location = use_location();
    Effect::new(move |_| {
        if wallet.wrong_chain() && location.pathname.get() != "/wrong-network" {
            navigate("/wrong-network", Default::default());
        }
    });
}

/// How the user wants to connect.
#[derive(Clone)]
enum Connector {
    /// A browser wallet: its EIP-6963 rdns, or None for plain `window.ethereum`.
    Browser(Option<String>),
    WalletConnect,
}

#[component]
fn WalletBar() -> impl IntoView {
    let wallet = Wallet::get();
    let error = RwSignal::new(None::<String>);
    let picking = RwSignal::new(false);

    let connect = move |connector: Connector| {
        picking.set(false);
        spawn_local(async move {
            let result = async {
                let address = match connector {
                    Connector::Browser(rdns) => eth::connect_injected(rdns).await?,
                    Connector::WalletConnect => eth::connect_walletconnect().await?,
                };
                // Ask the wallet to move to Sepolia right away (adds the chain if it doesn't know it).
                let mut switch_error = None;
                if eth::chain_id().await? != SEPOLIA_ID
                    && let Err(e) = eth::switch_to_sepolia().await
                {
                    switch_error = Some(friendly_error(&e));
                }
                let chain = eth::chain_id().await?;
                Ok::<_, String>((address, chain, switch_error))
            }
            .await;
            match result {
                Ok((address, chain, switch_error)) => {
                    wallet.address.set(Some(address));
                    wallet.chain_id.set(Some(chain));
                    error.set(switch_error);
                }
                Err(e) => error.set(Some(friendly_error(&e))),
            }
        })
    };
    let disconnect = move |_| {
        eth::disconnect();
        wallet.address.set(None);
        wallet.chain_id.set(None);
    };

    view! {
        <div class="wallet-bar">
            {move || match wallet.address.get() {
                None => view! {
                    <button on:click=move |_| picking.update(|p| *p = !*p)>"Connect browser wallet"</button>
                    <button on:click=move |_| connect(Connector::WalletConnect)>"WalletConnect (QR)"</button>
                }
                .into_any(),
                Some(address) => view! {
                    <AddressLabel address/>
                    <span class="chain">
                        {move || wallet.chain_id.get().map(|id| format!("chain {id}"))}
                    </span>
                    <Show when=move || wallet.wrong_chain()>
                        <SwitchSepoliaButton/>
                    </Show>
                    <button class="secondary" on:click=disconnect>"Disconnect"</button>
                }
                .into_any(),
            }}
            {move || error.get().map(|e| view! { <span class="error">{e}</span> })}
        </div>
        <Show when=move || picking.get() && wallet.address.get().is_none()>
            <div class="wallet-picker">
                <p class="muted">"Choose a wallet"</p>
                {move || {
                    let wallets = wallet.wallets.get();
                    if wallets.is_empty() {
                        view! {
                            <button class="secondary" on:click=move |_| connect(Connector::Browser(None))>
                                "Default browser wallet"
                            </button>
                        }
                        .into_any()
                    } else {
                        wallets
                            .into_iter()
                            .map(|info| {
                                let rdns = info.rdns.clone();
                                view! {
                                    <button
                                        class="secondary wallet-option"
                                        title=info.rdns
                                        on:click=move |_| connect(Connector::Browser(Some(rdns.clone())))
                                    >
                                        <img src=info.icon alt=""/>
                                        {info.name}
                                    </button>
                                }
                            })
                            .collect_view()
                            .into_any()
                    }
                }}
            </div>
        </Show>
    }
}

#[component]
pub fn SwitchSepoliaButton() -> impl IntoView {
    let wallet = Wallet::get();
    let error = RwSignal::new(None::<String>);
    let switch = move |_| {
        spawn_local(async move {
            let result = async {
                eth::switch_to_sepolia().await?;
                eth::chain_id().await
            }
            .await;
            match result {
                Ok(chain) => {
                    wallet.chain_id.set(Some(chain));
                    error.set(None);
                }
                Err(e) => error.set(Some(friendly_error(&e))),
            }
        })
    };
    view! {
        <button on:click=switch>"Switch to Sepolia"</button>
        {move || error.get().map(|e| view! { <span class="error">{e}</span> })}
    }
}

/// Shows the ENS name of `address` if it has one, else the short hex.
#[component]
pub fn AddressLabel(address: String) -> impl IntoView {
    let lookup = address.clone();
    let ens = LocalResource::new(move || {
        let lookup = lookup.clone();
        async move { eth::ens_name(&lookup).await }
    });
    let short = short_address(&address);
    view! {
        <span class="address" title=address>
            {move || match ens.get() {
                Some(Ok(Some(name))) => name,
                _ => short.clone(),
            }}
        </span>
    }
}

#[derive(Clone, PartialEq)]
enum TxState {
    Idle,
    AwaitingWallet,
    Pending(String),
    Confirmed(String),
    Reverted(String),
    Failed(String),
}

/// Button for every write call: wallet prompt, pending, success with Etherscan link, readable errors.
/// `action` sends the transaction and returns its hash.
#[component]
pub fn TxButton<F, Fut>(label: &'static str, action: F) -> impl IntoView
where
    F: Fn() -> Fut + Copy + 'static,
    Fut: Future<Output = Result<String, String>> + 'static,
{
    let state = RwSignal::new(TxState::Idle);
    let busy = move || matches!(state.get(), TxState::AwaitingWallet | TxState::Pending(_));
    let run = move |_| {
        state.set(TxState::AwaitingWallet);
        spawn_local(async move {
            let next = match action().await {
                Err(e) => TxState::Failed(friendly_error(&e)),
                Ok(hash) => {
                    state.set(TxState::Pending(hash.clone()));
                    match eth::wait_for_receipt(&hash).await {
                        Ok(true) => TxState::Confirmed(hash),
                        Ok(false) => TxState::Reverted(hash),
                        Err(e) => TxState::Failed(friendly_error(&e)),
                    }
                }
            };
            state.set(next);
        })
    };

    view! {
        <div class="tx">
            <button on:click=run disabled=busy>{label}</button>
            {move || match state.get() {
                TxState::Idle => ().into_any(),
                TxState::AwaitingWallet => {
                    view! { <span class="muted">"Confirm in your wallet…"</span> }.into_any()
                }
                TxState::Pending(hash) => {
                    view! { <span class="muted">"Pending… "</span> <TxLink hash/> }.into_any()
                }
                TxState::Confirmed(hash) => {
                    view! { <span class="ok">"Confirmed ✓ "</span> <TxLink hash/> }.into_any()
                }
                TxState::Reverted(hash) => {
                    view! { <span class="error">"Transaction reverted. "</span> <TxLink hash/> }
                        .into_any()
                }
                TxState::Failed(e) => view! { <span class="error">{e}</span> }.into_any(),
            }}
        </div>
    }
}

#[component]
fn TxLink(hash: String) -> impl IntoView {
    view! {
        <a href=format!("https://sepolia.etherscan.io/tx/{hash}") target="_blank" rel="noopener">
            "View on Etherscan"
        </a>
    }
}

/// Turns raw wallet / node errors into text a user understands.
pub fn friendly_error(raw: &str) -> String {
    let lower = raw.to_lowercase();
    if lower.contains("user rejected") || lower.contains("user denied") {
        "You cancelled the request in your wallet.".into()
    } else if lower.contains("insufficient funds") {
        "Not enough Sepolia ETH to pay for this transaction. Get some from a faucet.".into()
    } else if lower.contains("ethereum is not defined") {
        "No browser wallet found. Install MetaMask or use WalletConnect.".into()
    } else if lower.contains("execution reverted") {
        format!("The contract refused this transaction: {raw}")
    } else {
        raw.into()
    }
}

/// Renders a LocalResource value: loading text, the error, or `ok(value)`.
pub fn loaded<T, V>(value: Option<Result<T, String>>, ok: impl FnOnce(T) -> V) -> AnyView
where
    V: IntoView + 'static,
{
    match value {
        None => view! { <span class="muted">"Loading…"</span> }.into_any(),
        Some(Ok(v)) => ok(v).into_view().into_any(),
        Some(Err(e)) => view! { <span class="error">{e}</span> }.into_any(),
    }
}
