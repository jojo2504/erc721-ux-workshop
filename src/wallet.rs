//! Wallet state shared by every page (Leptos context).

use leptos::prelude::*;

use crate::eth;

pub const SEPOLIA_ID: u64 = 11_155_111;

#[derive(Clone, Copy)]
pub struct Wallet {
    pub address: RwSignal<Option<String>>,
    pub chain_id: RwSignal<Option<u64>>,
}

impl Wallet {
    /// Create the state, share it with all children, and keep it in sync with wallet events.
    pub fn provide() -> Self {
        let wallet = Self {
            address: RwSignal::new(None),
            chain_id: RwSignal::new(None),
        };
        provide_context(wallet);
        eth::subscribe(
            move |address| wallet.address.set(address),
            move |chain| wallet.chain_id.set(Some(chain)),
        );
        // Restore a connection the wallet already authorized (page reload, direct link).
        leptos::task::spawn_local(async move {
            if let Ok(Some(address)) = eth::current_account().await {
                wallet.chain_id.set(eth::chain_id().await.ok());
                wallet.address.set(Some(address));
            }
        });
        wallet
    }

    pub fn get() -> Self {
        use_context().expect("Wallet::provide() must run in App")
    }

    pub fn wrong_chain(&self) -> bool {
        matches!(self.chain_id.get(), Some(id) if id != SEPOLIA_ID)
    }
}

/// "0x1234…abcd"
pub fn short_address(address: &str) -> String {
    match (address.get(..6), address.get(address.len().saturating_sub(4)..)) {
        (Some(start), Some(end)) if address.len() > 10 => format!("{start}…{end}"),
        _ => address.to_string(),
    }
}

/// 1500000000000000 -> "0.0015 ETH"
pub fn format_eth(wei: u128) -> String {
    let unit = 10u128.pow(18);
    let text = format!("{}.{:018}", wei / unit, wei % unit);
    format!("{} ETH", text.trim_end_matches('0').trim_end_matches('.'))
}
