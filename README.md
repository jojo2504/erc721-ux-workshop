# ERC721 UX 101 — Leptos (Rust/WASM) frontend

Frontend for the [ERC721 UX 101](https://github.com/Elli610/Blockchain-101/tree/main/workshops/solidity/erc721-ux-101) workshop, written in Rust with [Leptos](https://leptos.dev) (CSR) and compiled to WebAssembly with [Trunk](https://trunkrs.dev). Contract calls are ABI-encoded with `alloy-sol-types`; no wagmi / RainbowKit / Web3Modal.

## Run

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk --locked --root .tools   # repo-local, see ./trunk
cp .env.example .env        # fill in RPC URLs and WalletConnect project id
(cd wc && npm install && npm run build)   # bundles @walletconnect/ethereum-provider into public/wc.js
./trunk serve --open
```

`.env` is read at build time by `build.rs`. Everything in it ends up in the browser bundle, so restrict the RPC key to your origin.

## Where each exercise lives

| Exercise | Code |
| --- | --- |
| RPC key (Sepolia + mainnet for ENS) | `.env.example`, `src/eth.rs` (`SEPOLIA_RPC`, `MAINNET_RPC`) |
| `submitExercise()` | `/` — `src/pages/home.rs` |
| `window.ethereum` connect, `accountsChanged` / `chainChanged` | `src/eth.rs` (`connect_injected`, `subscribe`), `src/wallet.rs` |
| WalletConnect v2 (QR, no modal library) | `wc/entry.js`, `src/eth.rs` (`connect_walletconnect`) |
| Switch to Sepolia with `wallet_addEthereumChain` fallback (4902) | `src/eth.rs` (`switch_to_sepolia`), `SwitchSepoliaButton` |
| `/chain-info`: chain id, block number, address | `src/pages/chain_info.rs` |
| Wrong network page + redirect | `src/pages/wrong_network.rs`, `ChainGuard` in `src/components.rs` |
| `/fakeBayc`: name, total supply, claim, `ex1` | `src/pages/fake_bayc.rs` |
| `/fakeBayc/:tokenId`: image + attributes, not-found state | `src/pages/fake_bayc.rs` (`FakeBaycToken`) |
| `/fakeBayc/gallery/:address` with lazy images | `src/pages/fake_bayc.rs` (`FakeBaycGallery`) |
| `/fakeNefturians`: price, `buyAToken()` payable, `ex2` | `src/pages/fake_nefturians.rs` |
| `/fakeNefturians/:address` | `src/pages/fake_nefturians.rs` (`FakeNefturiansOwned`) |
| `/fakeMeebits`: unclaimed ids, signature pick, claim, `ex3` | `src/pages/fake_meebits.rs`, `public/output-sig.json` |
| Tx lifecycle (wallet prompt, pending, Etherscan link, revert reason) | `TxButton` in `src/components.rs`, dry-run `eth_call` in `src/eth.rs` (`send`) |
| ENS reverse resolution (with forward check) | `src/eth.rs` (`ens_name`), `AddressLabel` |

## Extras

- Owned-token lists and Meebits claim status are read in one request through Multicall3.
- `ipfs://` URIs are resolved through several public gateways, each with a timeout fallback.
- `tokenURI` metadata is cached in memory.
- Dark mode (`prefers-color-scheme`) and responsive layout.
