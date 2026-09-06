# Anchor Vault

A personal SOL vault. Every user gets one, at an address derived from their own pubkey, and only they can move what's inside. Native lamports only, no mints, no token accounts: deposits and withdrawals are plain System program transfers.

```
user  (signer)
 └── vault_state   PDA ["state", user]         stores the owner and both bumps
      └── vault    PDA ["vault", vault_state]  a bare SystemAccount, holds the lamports
```

## Instructions

| | Args | What it does |
|---|---|---|
| `initialize` | | Creates `vault_state`, records the bumps, funds the vault to the rent-exempt minimum. |
| `deposit` | `amount` | Moves lamports from the user into the vault. |
| `withdraw` | `amount` | Moves lamports back out, signed by the vault PDA. |
| `close` | | Returns the whole balance, then closes `vault_state` so its rent comes back too. |

Every instruction takes the same four accounts: `user`, `vault_state`, `vault`, `system_program`.

## Notes for a reviewer

**The seeds are the authorization.** There is no ownership check in any handler and no `has_one`. `vault_state` is derived from the signer's key, so a caller can only ever address the PDA chain that hangs off their own pubkey. An attacker who signs with their own keypair and passes someone else's `vault_state` and `vault` fails `ConstraintSeeds` in validation, long before a lamport moves. The `user` field in state is there for clients reading the account, not as a guard.

**The vault is a `SystemAccount`, not an `Account`.** It carries lamports and no data, which is what lets the System program move value out of it under a PDA signature. So withdrawals are a `transfer` CPI with `signer_seeds` rather than hand-edited lamport balances, and the runtime does the arithmetic and the checks.

**Overdrawing needs no check of its own.** A withdrawal larger than the balance fails inside the System program. Adding a `require!` above it would only duplicate a check that already runs.

**A drained vault disappears, and that's fine.** `initialize` funds the vault to the rent-exempt minimum so it exists before the first deposit. Withdraw everything and the account drops to zero lamports and gets reaped; the next deposit recreates it at the same address, because the address is a function of the seeds and nothing else.

**Bumps are stored once, at init.** `find_program_address` searches downward from 255, so re-deriving on every instruction burns compute for a value that can never change. Later instructions pass `bump = vault_state.vault_bump`, which also pins the canonical bump instead of accepting any bump that happens to land off-curve.

There is no custom error enum. Failures come back as Anchor constraint errors or System program errors.

## Build and test

```sh
anchor build   # writes target/deploy/vault_new.so
cargo test     # integration tests, LiteSVM
```

Build first: the tests pull the `.so` in with `include_bytes!`. They run against LiteSVM rather than a validator, and cover the init round trip, deposit and withdraw, an overdraw, and one user trying to withdraw from another's vault.

## Driving it against a validator

`examples/cli.rs` is a small client for a deployed program. It derives both PDAs, builds one instruction, sends it, and prints balances after.

```sh
solana-test-validator
anchor deploy

cargo run --example cli -- addresses
cargo run --example cli -- init
cargo run --example cli -- deposit 2
cargo run --example cli -- balance
cargo run --example cli -- withdraw 1
cargo run --example cli -- close
```

Amounts are in SOL. `RPC_URL` and `KEYPAIR` override the defaults, which are localhost and `~/.config/solana/id.json`.

The program ID in `declare_id!` is a local keypair under `target/`, which is gitignored. A fresh clone builds its own; nothing here is deployed.
