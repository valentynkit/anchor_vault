//! Drives the vault against a running validator.
//!
//!   cargo run --example cli -- addresses
//!   cargo run --example cli -- init
//!   cargo run --example cli -- deposit 2
//!   cargo run --example cli -- balance
//!   cargo run --example cli -- withdraw 1
//!   cargo run --example cli -- close
//!
//! RPC_URL and KEYPAIR override the defaults (localhost, ~/.config/solana/id.json).

use {
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, system_program},
        AccountDeserialize, InstructionData, ToAccountMetas,
    },
    solana_rpc_client::rpc_client::RpcClient,
    solana_commitment_config::CommitmentConfig,
    solana_keypair::{read_keypair_file, Keypair},
    solana_signer::Signer,
    solana_transaction_v4::Transaction,
    vault_new::{
        constants::{STATE_SEED, VAULT_SEED},
        state::VaultState,
    },
};

const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

type Res = Result<(), Box<dyn std::error::Error>>;

fn main() -> Res {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("help");

    let url = std::env::var("RPC_URL").unwrap_or_else(|_| "http://127.0.0.1:8899".into());
    let keypair = std::env::var("KEYPAIR")
        .unwrap_or_else(|_| format!("{}/.config/solana/id.json", std::env::var("HOME").unwrap()));

    let payer = read_keypair_file(&keypair)?;
    let rpc = RpcClient::new_with_commitment(url, CommitmentConfig::confirmed());

    let pid = vault_new::id();
    let user = payer.pubkey();
    let state = Pubkey::find_program_address(&[STATE_SEED, user.as_ref()], &pid).0;
    let vault = Pubkey::find_program_address(&[VAULT_SEED, state.as_ref()], &pid).0;

    // Same four accounts for every instruction, so one meta list covers all of them.
    let metas = vault_new::accounts::Initialize {
        user,
        vault_state: state,
        vault,
        system_program: system_program::ID,
    }
    .to_account_metas(None);

    let amount = || -> u64 {
        let sol: f64 = args
            .get(1)
            .and_then(|s| s.parse().ok())
            .expect("usage: cli <deposit|withdraw> <SOL>");
        (sol * LAMPORTS_PER_SOL) as u64
    };

    let data = match cmd {
        "init" => vault_new::instruction::Initialize {}.data(),
        "deposit" => vault_new::instruction::Deposit { amount: amount() }.data(),
        "withdraw" => vault_new::instruction::Withdraw { amount: amount() }.data(),
        "close" => vault_new::instruction::Close {}.data(),
        "addresses" => {
            println!("program     {pid}");
            println!("user        {user}");
            println!("vault_state {state}");
            println!("vault       {vault}");
            return Ok(());
        }
        "balance" => return balance(&rpc, &user, &state, &vault),
        _ => {
            eprintln!("usage: cli <addresses|init|deposit SOL|withdraw SOL|balance|close>");
            std::process::exit(2);
        }
    };

    let sig = send(&rpc, &payer, Instruction::new_with_bytes(pid, &data, metas))?;
    println!("ok  {sig}");
    println!("    solana confirm -v {sig} -u {}", rpc.url());
    balance(&rpc, &user, &state, &vault)
}

fn send(
    rpc: &RpcClient,
    payer: &Keypair,
    ix: Instruction,
) -> Result<String, Box<dyn std::error::Error>> {
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[payer],
        rpc.get_latest_blockhash()?,
    );
    Ok(rpc.send_and_confirm_transaction(&tx)?.to_string())
}

fn balance(rpc: &RpcClient, user: &Pubkey, state: &Pubkey, vault: &Pubkey) -> Res {
    let sol = |p: &Pubkey| rpc.get_balance(p).unwrap_or(0) as f64 / LAMPORTS_PER_SOL;
    println!("user        {:>16.9} SOL", sol(user));
    println!("vault       {:>16.9} SOL", sol(vault));

    match rpc.get_account(state) {
        Ok(acct) => {
            let st = VaultState::try_deserialize(&mut acct.data.as_slice())?;
            println!(
                "vault_state {:>16.9} SOL  user={} vault_bump={} state_bump={}",
                acct.lamports as f64 / LAMPORTS_PER_SOL,
                st.user,
                st.vault_bump,
                st.state_bump
            );
        }
        Err(_) => println!("vault_state      (does not exist)"),
    }
    Ok(())
}
