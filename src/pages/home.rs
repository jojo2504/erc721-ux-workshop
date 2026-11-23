use leptos::prelude::*;

use crate::{components::TxButton, eth};

#[component]
pub fn Home() -> impl IntoView {
    view! {
        <h1>"ERC721 UX 101"</h1>
        <section>
            <h2>"Register"</h2>
            <p>"Bind your wallet to the Evaluator. Do this once, with the wallet you use for the whole TD."</p>
            <TxButton label="submitExercise()" action=eth::submit_exercise/>
        </section>
    }
}
