use anchor_lang::{
    prelude::*,
    system_program::{transfer, Transfer},
};

use crate::{constants::*, state::VaultState};

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(
        init,
        payer = user,
        space = VaultState::DISCRIMINATOR.len() + VaultState::INIT_SPACE,
        seeds = [STATE_SEED, user.key().as_ref()],
        bump
    )]
    pub vault_state: Account<'info, VaultState>,
    #[account(
        mut,
        seeds = [VAULT_SEED, vault_state.key().as_ref()],
        bump
    )]
    pub vault: SystemAccount<'info>,
    pub system_program: Program<'info, System>,
}

impl<'info> Initialize<'info> {
    pub fn initialize(&mut self, bumps: &InitializeBumps) -> Result<()> {
        self.vault_state.set_inner(VaultState {
            user: self.user.key(),
            vault_bump: bumps.vault,
            state_bump: bumps.vault_state,
        });

        // Fund the vault to the rent-exempt minimum so it exists before any deposit.
        // ponytail: a full withdraw drains it back to zero and the account disappears;
        // the next deposit recreates it at the same address, so nothing breaks.
        let rent_exempt = Rent::get()?.minimum_balance(0);
        transfer(
            CpiContext::new(
                System::id(),
                Transfer {
                    from: self.user.to_account_info(),
                    to: self.vault.to_account_info(),
                },
            ),
            rent_exempt,
        )
    }
}
