use leptos::prelude::*;
use leptos_router::hooks::use_navigate;

use crate::{
    components::SwitchSepoliaButton,
    wallet::{SEPOLIA_ID, Wallet},
};

#[component]
pub fn WrongNetwork() -> impl IntoView {
    let wallet = Wallet::get();
    let navigate = use_navigate();
    // Back home as soon as the wallet is on Sepolia.
    Effect::new(move |_| {
        if wallet.chain_id.get() == Some(SEPOLIA_ID) {
            navigate("/", Default::default());
        }
    });

    view! {
        <div class="error-state">
            <h1>"Wrong network"</h1>
            <p>
                "Your wallet is on chain "
                {move || wallet.chain_id.get().map(|id| id.to_string())}
                ". This app only works on Sepolia (11155111)."
            </p>
            <SwitchSepoliaButton/>
        </div>
    }
}
