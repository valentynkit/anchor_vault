use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, system_program},
        AccountDeserialize, InstructionData, ToAccountMetas,
    },
    litesvm::{types::TransactionResult, LiteSVM},
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
    vault_new::{
        constants::{STATE_SEED, VAULT_SEED},
        state::VaultState,
    },
};

const PROGRAM_BYTES: &[u8] = include_bytes!(concat!(
    env!("CARGO_TARGET_TMPDIR"),
    "/../deploy/vault_new.so"
));

const SOL: u64 = 1_000_000_000;

struct Pdas {
    state: Pubkey,
    vault: Pubkey,
}

fn pdas(user: &Pubkey) -> Pdas {
    let program_id = vault_new::id();
    let state = Pubkey::find_program_address(&[STATE_SEED, user.as_ref()], &program_id).0;
    let vault = Pubkey::find_program_address(&[VAULT_SEED, state.as_ref()], &program_id).0;
    Pdas { state, vault }
}

fn setup() -> (LiteSVM, Keypair) {
    let mut svm = LiteSVM::new();
    svm.add_program(vault_new::id(), PROGRAM_BYTES).unwrap();
    let user = Keypair::new();
    svm.airdrop(&user.pubkey(), 10 * SOL).unwrap();
    (svm, user)
}

fn send(svm: &mut LiteSVM, ix: Instruction, payer: &Keypair) -> TransactionResult {
    let msg = Message::new_with_blockhash(&[ix], Some(&payer.pubkey()), &svm.latest_blockhash());
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[payer]).unwrap();
    svm.send_transaction(tx)
}

fn init_ix(user: &Pubkey) -> Instruction {
    let p = pdas(user);
    Instruction::new_with_bytes(
        vault_new::id(),
        &vault_new::instruction::Initialize {}.data(),
        vault_new::accounts::Initialize {
            user: *user,
            vault_state: p.state,
            vault: p.vault,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

/// `state_owner` selects whose PDAs are passed; `signer` selects who signs.
/// Equal for the happy path, different to reproduce the drain attempt.
fn deposit_ix(signer: &Pubkey, state_owner: &Pubkey, amount: u64) -> Instruction {
    let p = pdas(state_owner);
    Instruction::new_with_bytes(
        vault_new::id(),
        &vault_new::instruction::Deposit { amount }.data(),
        vault_new::accounts::Deposit {
            user: *signer,
            vault_state: p.state,
            vault: p.vault,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn withdraw_ix(signer: &Pubkey, state_owner: &Pubkey, amount: u64) -> Instruction {
    let p = pdas(state_owner);
    Instruction::new_with_bytes(
        vault_new::id(),
        &vault_new::instruction::Withdraw { amount }.data(),
        vault_new::accounts::Withdraw {
            user: *signer,
            vault_state: p.state,
            vault: p.vault,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn read_state(svm: &LiteSVM, state: &Pubkey) -> VaultState {
    let acct = svm.get_account(state).unwrap();
    VaultState::try_deserialize(&mut acct.data.as_slice()).unwrap()
}

#[test]
fn initialize_stores_bumps_and_funds_vault() {
    let (mut svm, user) = setup();
    let p = pdas(&user.pubkey());

    send(&mut svm, init_ix(&user.pubkey()), &user).unwrap();

    let state = read_state(&svm, &p.state);
    assert_eq!(state.user, user.pubkey());

    // stored bumps must match what the client derives, or every later
    // `bump = vault_state.*_bump` check fails
    let (_, state_bump) =
        Pubkey::find_program_address(&[STATE_SEED, user.pubkey().as_ref()], &vault_new::id());
    let (_, vault_bump) =
        Pubkey::find_program_address(&[VAULT_SEED, p.state.as_ref()], &vault_new::id());
    assert_eq!(state.state_bump, state_bump);
    assert_eq!(state.vault_bump, vault_bump);

    assert_eq!(
        svm.get_balance(&p.vault).unwrap(),
        svm.minimum_balance_for_rent_exemption(0),
        "vault should be funded to rent-exempt minimum at init"
    );
}

#[test]
fn deposit_then_withdraw_round_trips() {
    let (mut svm, user) = setup();
    let p = pdas(&user.pubkey());
    send(&mut svm, init_ix(&user.pubkey()), &user).unwrap();

    let base = svm.get_balance(&p.vault).unwrap();

    send(
        &mut svm,
        deposit_ix(&user.pubkey(), &user.pubkey(), 2 * SOL),
        &user,
    )
    .unwrap();
    assert_eq!(svm.get_balance(&p.vault).unwrap(), base + 2 * SOL);

    // asserting on the vault, not the user: the user also pays tx fees
    let before = svm.get_balance(&user.pubkey()).unwrap();
    send(
        &mut svm,
        withdraw_ix(&user.pubkey(), &user.pubkey(), 2 * SOL),
        &user,
    )
    .unwrap();
    assert_eq!(svm.get_balance(&p.vault).unwrap(), base);
    assert!(svm.get_balance(&user.pubkey()).unwrap() > before);
}

#[test]
fn withdraw_from_another_users_vault_fails() {
    let (mut svm, victim) = setup();
    let attacker = Keypair::new();
    svm.airdrop(&attacker.pubkey(), 10 * SOL).unwrap();

    send(&mut svm, init_ix(&victim.pubkey()), &victim).unwrap();
    send(
        &mut svm,
        deposit_ix(&victim.pubkey(), &victim.pubkey(), 5 * SOL),
        &victim,
    )
    .unwrap();

    let victim_pdas = pdas(&victim.pubkey());
    let funded = svm.get_balance(&victim_pdas.vault).unwrap();

    // attacker signs, but passes the victim's vault_state + vault
    let res = send(
        &mut svm,
        withdraw_ix(&attacker.pubkey(), &victim.pubkey(), funded),
        &attacker,
    );

    let err = format!("{:?}", res.unwrap_err());
    assert!(
        err.contains("ConstraintSeeds") || err.contains("2006"),
        "expected a seeds-constraint failure, got: {err}"
    );
    assert_eq!(
        svm.get_balance(&victim_pdas.vault).unwrap(),
        funded,
        "victim's vault must be untouched"
    );
}

#[test]
fn withdraw_more_than_balance_fails() {
    let (mut svm, user) = setup();
    let p = pdas(&user.pubkey());
    send(&mut svm, init_ix(&user.pubkey()), &user).unwrap();
    send(
        &mut svm,
        deposit_ix(&user.pubkey(), &user.pubkey(), SOL),
        &user,
    )
    .unwrap();

    let balance = svm.get_balance(&p.vault).unwrap();
    let res = send(
        &mut svm,
        withdraw_ix(&user.pubkey(), &user.pubkey(), balance + SOL),
        &user,
    );

    assert!(res.is_err(), "overdraw must fail");
    assert_eq!(svm.get_balance(&p.vault).unwrap(), balance);
}
