mod components;
mod eth;
mod pages;
mod wallet;

use leptos::prelude::*;
use leptos_router::{
    components::{Route, Router, Routes},
    path,
};

use crate::{components::Header, pages::*, wallet::Wallet};

#[component]
fn App() -> impl IntoView {
    Wallet::provide();
    view! {
        <Router>
            <Header/>
            <main>
                <Routes fallback=|| view! { <h1>"Page not found"</h1> }>
                    <Route path=path!("/") view=Home/>
                    <Route path=path!("/chain-info") view=ChainInfo/>
                    <Route path=path!("/wrong-network") view=WrongNetwork/>
                    <Route path=path!("/fakeBayc") view=FakeBayc/>
                    <Route path=path!("/fakeBayc/gallery/:address") view=FakeBaycGallery/>
                    <Route path=path!("/fakeBayc/:token_id") view=FakeBaycToken/>
                    <Route path=path!("/fakeNefturians") view=FakeNefturians/>
                    <Route path=path!("/fakeNefturians/:address") view=FakeNefturiansOwned/>
                    <Route path=path!("/fakeMeebits") view=FakeMeebits/>
                </Routes>
            </main>
        </Router>
    }
}

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}
