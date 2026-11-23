use leptos::prelude::*;
use leptos_router::{components::A, hooks::use_params_map};

use crate::{
    components::{TxButton, loaded},
    eth::{self, TokenMetadata},
    wallet::Wallet,
};

/// /fakeBayc
#[component]
pub fn FakeBayc() -> impl IntoView {
    let wallet = Wallet::get();
    let name = LocalResource::new(eth::bayc_name);
    let total = LocalResource::new(eth::bayc_total_supply);
    let token_input = RwSignal::new(String::new());

    view! {
        <h1>{move || loaded(name.get(), |n| n)}</h1>
        <p>"Tokens minted: " {move || loaded(total.get(), |n| n.to_string())}</p>

        <section>
            <h2>"Claim"</h2>
            <TxButton label="Claim a FakeBAYC" action=eth::bayc_claim/>
            <TxButton label="ex1_claimedFakeBAYC()" action=eth::ex1_claimed_fake_bayc/>
        </section>

        <section>
            <h2>"Browse"</h2>
            <input type="number" min="0" placeholder="Token id" bind:value=token_input/>
            <A href=move || format!("/fakeBayc/{}", token_input.get())>"View token"</A>
            {move || {
                wallet
                    .address
                    .get()
                    .map(|a| view! { <p><A href=format!("/fakeBayc/gallery/{a}")>"My gallery"</A></p> })
            }}
        </section>
    }
}

/// /fakeBayc/:token_id
#[component]
pub fn FakeBaycToken() -> impl IntoView {
    let params = use_params_map();
    let raw_id = move || params.read().get("token_id").unwrap_or_default();
    let meta = LocalResource::new(move || {
        let id = raw_id().parse::<u64>().ok();
        async move {
            match id {
                Some(id) => eth::bayc_metadata(id).await,
                None => Ok(None), // not a number: treat as "does not exist"
            }
        }
    });

    view! {
        <A href="/fakeBayc">"← FakeBAYC"</A>
        <h1>"FakeBAYC #" {raw_id}</h1>
        {move || loaded(meta.get(), move |m| match m {
            Some(meta) => view! { <Metadata meta/> }.into_any(),
            None => view! {
                <div class="error-state">
                    <h2>"Token not found"</h2>
                    <p>"FakeBAYC #" {raw_id()} " does not exist (yet)."</p>
                </div>
            }
            .into_any(),
        })}
    }
}

#[component]
fn Metadata(meta: TokenMetadata) -> impl IntoView {
    view! {
        <div class="token">
            <img src=meta.image alt="token image"/>
            <table>
                {meta
                    .attributes
                    .into_iter()
                    .map(|attr| {
                        let value = match attr.value {
                            serde_json::Value::String(s) => s,
                            other => other.to_string(),
                        };
                        view! { <tr><th>{attr.trait_type}</th><td>{value}</td></tr> }
                    })
                    .collect_view()}
            </table>
        </div>
    }
}

/// /fakeBayc/gallery/:address
#[component]
pub fn FakeBaycGallery() -> impl IntoView {
    let params = use_params_map();
    let address = move || params.read().get("address").unwrap_or_default();
    let tokens = LocalResource::new(move || {
        let address = address();
        async move { eth::bayc_tokens_of(&address).await }
    });

    view! {
        <h1>"FakeBAYC gallery"</h1>
        <p><code>{address}</code></p>
        {move || loaded(tokens.get(), |ids| {
            if ids.is_empty() {
                view! { <p class="muted">"This address owns no FakeBAYC."</p> }.into_any()
            } else {
                view! {
                    <div class="grid">
                        {ids.into_iter().map(|id| view! { <GalleryItem id/> }).collect_view()}
                    </div>
                }
                .into_any()
            }
        })}
    }
}

#[component]
fn GalleryItem(id: u64) -> impl IntoView {
    let meta = LocalResource::new(move || eth::bayc_metadata(id));
    view! {
        <A href=format!("/fakeBayc/{id}")>
            <figure>
                {move || loaded(meta.get(), |m| {
                    m.map(|m| view! { <img src=m.image loading="lazy" alt=format!("FakeBAYC #{id}")/> })
                })}
                <figcaption>"#" {id}</figcaption>
            </figure>
        </A>
    }
}
