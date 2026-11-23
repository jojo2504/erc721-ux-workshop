use leptos::prelude::*;
use leptos_router::{components::A, hooks::use_params_map};

use crate::{
    components::{TxButton, loaded},
    eth,
    wallet::{Wallet, format_eth},
};

/// /fakeNefturians
#[component]
pub fn FakeNefturians() -> impl IntoView {
    let wallet = Wallet::get();
    let price = LocalResource::new(eth::nefturians_price_wei);

    view! {
        <h1>"FakeNefturians"</h1>
        <p>"Price: " {move || loaded(price.get(), format_eth)}</p>
        <TxButton label="Buy a FakeNefturian" action=eth::nefturians_buy/>
        <TxButton label="ex2_boughtFakeNefturians()" action=eth::ex2_bought_fake_nefturians/>
        {move || {
            wallet
                .address
                .get()
                .map(|a| view! { <p><A href=format!("/fakeNefturians/{a}")>"My FakeNefturians"</A></p> })
        }}
    }
}

/// /fakeNefturians/:address
#[component]
pub fn FakeNefturiansOwned() -> impl IntoView {
    let params = use_params_map();
    let address = move || params.read().get("address").unwrap_or_default();
    let tokens = LocalResource::new(move || {
        let address = address();
        async move { eth::nefturians_tokens_of(&address).await }
    });

    view! {
        <A href="/fakeNefturians">"← FakeNefturians"</A>
        <h1>"FakeNefturians owned"</h1>
        <p><code>{address}</code></p>
        {move || loaded(tokens.get(), |ids| {
            if ids.is_empty() {
                view! { <p class="muted">"This address owns no FakeNefturians."</p> }.into_any()
            } else {
                view! { <ul>{ids.into_iter().map(|id| view! { <li>"#" {id}</li> }).collect_view()}</ul> }
                    .into_any()
            }
        })}
    }
}
