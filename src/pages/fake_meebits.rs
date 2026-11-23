use leptos::prelude::*;

use crate::{
    components::{TxButton, loaded},
    eth,
};

/// /fakeMeebits
#[component]
pub fn FakeMeebits() -> impl IntoView {
    let unclaimed = LocalResource::new(eth::meebits_unclaimed_ids);
    let selected = RwSignal::new(None::<u64>);
    let signature = LocalResource::new(move || {
        let id = selected.get();
        async move {
            match id {
                Some(id) => eth::meebits_signature(id).await.map(Some),
                None => Ok(None),
            }
        }
    });
    let claim = move || async move {
        let id = selected.get_untracked().ok_or("Pick a token id first")?;
        let signature = eth::meebits_signature(id).await?;
        eth::meebits_claim(id, &signature).await
    };

    view! {
        <h1>"FakeMeebits"</h1>
        <section>
            <h2>"Pick an unclaimed token"</h2>
            {move || loaded(unclaimed.get(), move |ids| view! {
                <p class="muted">{ids.len()} " tokens left"</p>
                <select on:change=move |ev| selected.set(event_target_value(&ev).parse().ok())>
                    <option value="">"Choose a token id"</option>
                    {ids
                        .into_iter()
                        .map(|id| view! { <option value=id.to_string()>{id}</option> })
                        .collect_view()}
                </select>
            })}
            <p>
                "Signature: "
                {move || loaded(signature.get(), |sig| match sig {
                    Some(sig) => view! { <code class="sig">{sig}</code> }.into_any(),
                    None => view! { <span class="muted">"pick a token"</span> }.into_any(),
                })}
            </p>
        </section>
        <section>
            <TxButton label="Claim with signature" action=claim/>
            <TxButton label="ex3_claimedFakeMeebits()" action=eth::ex3_claimed_fake_meebits/>
        </section>
    }
}
