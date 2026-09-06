use anchor_lang::{
    prelude::*,
    system_program::{transfer, Transfer},
};

use crate::{constants::*, state::VaultState};

#[derive(Accounts)]
pub struct Close<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(
        mut,
        seeds = [STATE_SEED, user.key().as_ref()],
        bump = vault_state.state_bump,
        close = user,
    )]
    pub vault_state: Account<'info, VaultState>,
    #[account(mut,
    seeds = [VAULT_SEED, vault_state.key().as_ref()],
    bump = vault_state.vault_bump)]
    pub vault: SystemAccount<'info>,
    pub system_program: Program<'info, System>,
}
impl<'info> Close<'info> {
    pub fn close(&mut self) -> Result<()> {
        let transfer_accounts = Transfer {
            from: self.vault.to_account_info(),
            to: self.user.to_account_info(),
        };

        let seeds = &[
            VAULT_SEED,
            self.vault_state.to_account_info().key.as_ref(),
            &[self.vault_state.vault_bump],
        ];

        let signer_seeds = &[&seeds[..]];
        let cpi_ctx = CpiContext::new_with_signer(System::id(), transfer_accounts, signer_seeds);

        let lamports = self.vault.lamports();
        transfer(cpi_ctx, lamports)?;
        AccountsClose::close(&self.vault_state, self.user.to_account_info())?;
        Ok(())
    }
}
