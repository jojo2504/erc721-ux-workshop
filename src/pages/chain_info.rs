use leptos::prelude::*;

use crate::{
    components::{AddressLabel, loaded},
    eth,
    wallet::Wallet,
};

#[component]
pub fn ChainInfo() -> impl IntoView {
    let wallet = Wallet::get();
    let block = LocalResource::new(move || {
        wallet.chain_id.track(); // refetch when the network changes
        eth::block_number()
    });

    view! {
        <h1>"Chain info"</h1>
        <Show
            when=move || wallet.address.get().is_some()
            fallback=|| view! { <p class="muted">"Connect your wallet to see chain info."</p> }
        >
            <dl>
                <dt>"Chain id"</dt>
                <dd>{move || wallet.chain_id.get().map(|id| id.to_string())}</dd>
                <dt>"Latest block"</dt>
                <dd>
                    {move || loaded(block.get(), |n| n.to_string())}
                    " "
                    <button class="secondary" on:click=move |_| block.refetch()>"Refresh"</button>
                </dd>
                <dt>"Address"</dt>
                <dd>
                    {move || wallet.address.get().map(|address| view! { <AddressLabel address/> })}
                    <code>{move || wallet.address.get()}</code>
                </dd>
            </dl>
        </Show>
    }
}
