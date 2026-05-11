use anchor_lang::prelude::*;

declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

const MAX_GUARDIANS: usize = 5;
const LAMPORTS_PER_SOL: u64 = 1_000_000_000;
const DURESS_TIMELOCK: i64 = 30 * 24 * 3600;
const GUARDIAN_CLAIM_TIMELOCK: i64 = 48 * 3600;

fn timelock_for_amount(lamports: u64) -> i64 {
    let sol = lamports / LAMPORTS_PER_SOL;
    if sol == 0       { 3_600 }
    else if sol < 10  { 6 * 3_600 }
    else if sol < 100 { 3 * 24 * 3_600 }
    else              { 14 * 24 * 3_600 }
}

#[program]
pub mod sol_guard {
    use super::*;

    pub fn initialize_vault(
        ctx: Context<InitializeVault>,
        guardians: Vec<Pubkey>,
        guardian_threshold: u8,
        duress_key: Pubkey,
        heartbeat_interval: i64,
    ) -> Result<()> {
        require!(!guardians.is_empty() && guardians.len() <= MAX_GUARDIANS,
            SolGuardError::InvalidGuardianCount);
        require!(
            guardian_threshold >= 1 && guardian_threshold as usize <= guardians.len(),
            SolGuardError::InvalidThreshold
        );
        require!(heartbeat_interval > 0, SolGuardError::InvalidHeartbeatInterval);

        for i in 0..guardians.len() {
            for j in (i + 1)..guardians.len() {
                require!(guardians[i] != guardians[j], SolGuardError::DuplicateGuardian);
            }
        }

        let vault = &mut ctx.accounts.vault;
        vault.owner              = ctx.accounts.owner.key();
        vault.duress_key         = duress_key;
        vault.guardians          = guardians;
        vault.guardian_threshold = guardian_threshold;
        vault.last_heartbeat     = Clock::get()?.unix_timestamp;
        vault.heartbeat_interval = heartbeat_interval;
        vault.status             = VaultStatus::Active;
        vault.pending_withdrawal = None;
        vault.pending_proposal   = None;
        vault.bump               = ctx.bumps.vault;

        let vault_key = vault.key();
        let owner_key = vault.owner;
        let guardian_count = vault.guardians.len() as u8;
        emit!(VaultCreatedEvent { vault: vault_key, owner: owner_key, guardian_count });
        Ok(())
    }

    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        require!(amount > 0, SolGuardError::InvalidAmount);
        let vault_key = ctx.accounts.vault.key();
        anchor_lang::system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                anchor_lang::system_program::Transfer {
                    from: ctx.accounts.depositor.to_account_info(),
                    to:   ctx.accounts.vault.to_account_info(),
                },
            ),
            amount,
        )?;
        emit!(DepositEvent { vault: vault_key, amount });
        Ok(())
    }

    pub fn heartbeat(ctx: Context<OwnerOnly>) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let vault_key = ctx.accounts.vault.key();
        let owner_key = ctx.accounts.owner.key();
        ctx.accounts.vault.last_heartbeat = now;
        emit!(HeartbeatEvent { vault: vault_key, owner: owner_key, timestamp: now });
        Ok(())
    }

    pub fn initiate_withdrawal(
        ctx: Context<SignerOnVault>,
        amount: u64,
        destination: Pubkey,
    ) -> Result<()> {
        require!(ctx.accounts.vault.status == VaultStatus::Active, SolGuardError::VaultFrozen);
        require!(ctx.accounts.vault.pending_withdrawal.is_none(), SolGuardError::WithdrawalAlreadyPending);
        require!(amount > 0, SolGuardError::InvalidAmount);

        let signer    = ctx.accounts.signer.key();
        let is_owner  = signer == ctx.accounts.vault.owner;
        let is_duress = signer == ctx.accounts.vault.duress_key;
        require!(is_owner || is_duress, SolGuardError::Unauthorized);

        let rent_exempt = Rent::get()?.minimum_balance(Vault::SPACE);
        let balance     = ctx.accounts.vault.to_account_info().lamports();
        require!(balance > rent_exempt, SolGuardError::InsufficientFunds);
        let available = balance - rent_exempt;
        require!(amount < available, SolGuardError::CannotWithdrawFullBalance);

        let timelock = if is_duress { DURESS_TIMELOCK } else { timelock_for_amount(amount) };
        let now      = Clock::get()?.unix_timestamp;

        let vault     = &mut ctx.accounts.vault;
        let vault_key = vault.key();
        vault.pending_withdrawal = Some(PendingWithdrawal {
            amount,
            destination,
            initiated_at: now,
            timelock_duration: timelock,
        });

        emit!(WithdrawalInitiatedEvent {
            vault: vault_key,
            amount,
            destination,
            unlock_at: now + timelock,
            is_duress,
        });
        Ok(())
    }

    pub fn execute_withdrawal(ctx: Context<ExecuteWithdrawal>) -> Result<()> {
        let amount;
        let dest_key;
        {
            let vault   = &ctx.accounts.vault;
            require!(vault.status == VaultStatus::Active, SolGuardError::VaultFrozen);
            let pending = vault.pending_withdrawal.as_ref().ok_or(SolGuardError::NoPendingWithdrawal)?;
            let now     = Clock::get()?.unix_timestamp;
            require!(now >= pending.initiated_at + pending.timelock_duration, SolGuardError::TimelockNotExpired);
            require!(ctx.accounts.destination.key() == pending.destination, SolGuardError::WrongDestination);
            amount   = pending.amount;
            dest_key = pending.destination;
        }

        let vault_key = ctx.accounts.vault.key();
        **ctx.accounts.vault.to_account_info().try_borrow_mut_lamports()? -= amount;
        **ctx.accounts.destination.try_borrow_mut_lamports()?             += amount;
        ctx.accounts.vault.pending_withdrawal = None;

        emit!(WithdrawalExecutedEvent { vault: vault_key, amount, destination: dest_key });
        Ok(())
    }

    pub fn cancel_withdrawal(ctx: Context<SignerOnVault>) -> Result<()> {
        require!(ctx.accounts.vault.pending_withdrawal.is_some(), SolGuardError::NoPendingWithdrawal);
        let signer = ctx.accounts.signer.key();
        require!(
            signer == ctx.accounts.vault.owner || ctx.accounts.vault.guardians.contains(&signer),
            SolGuardError::Unauthorized
        );
        let vault_key = ctx.accounts.vault.key();
        ctx.accounts.vault.pending_withdrawal = None;
        emit!(WithdrawalCancelledEvent { vault: vault_key, cancelled_by: signer });
        Ok(())
    }

    pub fn emergency_freeze(ctx: Context<SignerOnVault>) -> Result<()> {
        let signer = ctx.accounts.signer.key();
        require!(
            signer == ctx.accounts.vault.owner || ctx.accounts.vault.guardians.contains(&signer),
            SolGuardError::Unauthorized
        );
        let vault_key = ctx.accounts.vault.key();
        ctx.accounts.vault.status             = VaultStatus::Frozen;
        ctx.accounts.vault.pending_withdrawal = None;
        emit!(VaultFrozenEvent { vault: vault_key, frozen_by: signer });
        Ok(())
    }

    pub fn owner_unfreeze(ctx: Context<OwnerOnly>) -> Result<()> {
        let vault_key = ctx.accounts.vault.key();
        ctx.accounts.vault.status = VaultStatus::Active;
        emit!(VaultUnfrozenEvent { vault: vault_key });
        Ok(())
    }

    pub fn create_guardian_proposal(
        ctx: Context<SignerOnVault>,
        action: ProposalActionType,
        action_data: Pubkey,
    ) -> Result<()> {
        let signer = ctx.accounts.signer.key();
        require!(ctx.accounts.vault.guardians.contains(&signer), SolGuardError::NotAGuardian);

        let vault_key  = ctx.accounts.vault.key();
        let created_at = Clock::get()?.unix_timestamp;
        ctx.accounts.vault.pending_proposal = Some(GuardianProposalData {
            action,
            action_data,
            approvals: vec![signer],
            created_at,
        });

        emit!(ProposalCreatedEvent { vault: vault_key, proposer: signer, action });
        Ok(())
    }

    pub fn approve_guardian_proposal(ctx: Context<SignerOnVault>) -> Result<()> {
        let signer = ctx.accounts.signer.key();
        require!(ctx.accounts.vault.guardians.contains(&signer), SolGuardError::NotAGuardian);

        let proposal = ctx.accounts.vault.pending_proposal.as_ref()
            .ok_or(SolGuardError::NoProposal)?;
        require!(!proposal.approvals.contains(&signer), SolGuardError::AlreadyApproved);

        let vault_key = ctx.accounts.vault.key();
        let proposal  = ctx.accounts.vault.pending_proposal.as_mut().unwrap();
        proposal.approvals.push(signer);
        let total = proposal.approvals.len() as u8;

        emit!(ProposalApprovedEvent { vault: vault_key, approver: signer, total_approvals: total });
        Ok(())
    }

    pub fn execute_guardian_proposal(ctx: Context<ExecuteGuardianProposal>) -> Result<()> {
        let action;
        let action_data;
        let created_at;
        {
            let vault    = &ctx.accounts.vault;
            let proposal = vault.pending_proposal.as_ref().ok_or(SolGuardError::NoProposal)?;
            require!(
                proposal.approvals.len() >= vault.guardian_threshold as usize,
                SolGuardError::InsufficientApprovals
            );
            action      = proposal.action;
            action_data = proposal.action_data;
            created_at  = proposal.created_at;
        }

        let now      = Clock::get()?.unix_timestamp;
        let vault_key = ctx.accounts.vault.key();

        match action {
            ProposalActionType::RotateOwner => {
                require!(ctx.accounts.vault.status == VaultStatus::Frozen, SolGuardError::VaultMustBeFrozen);
                ctx.accounts.vault.owner            = action_data;
                ctx.accounts.vault.status           = VaultStatus::Active;
                ctx.accounts.vault.pending_proposal = None;
                emit!(OwnerRotatedEvent { vault: vault_key, new_owner: action_data });
            }
            ProposalActionType::GuardianClaim => {
                require!(
                    now > ctx.accounts.vault.last_heartbeat + ctx.accounts.vault.heartbeat_interval,
                    SolGuardError::OwnerStillActive
                );
                require!(now >= created_at + GUARDIAN_CLAIM_TIMELOCK, SolGuardError::TimelockNotExpired);
                require!(ctx.accounts.destination.key() == action_data, SolGuardError::WrongDestination);

                let rent_exempt = Rent::get()?.minimum_balance(Vault::SPACE);
                let amount = ctx.accounts.vault.to_account_info().lamports().saturating_sub(rent_exempt);

                **ctx.accounts.vault.to_account_info().try_borrow_mut_lamports()? -= amount;
                **ctx.accounts.destination.try_borrow_mut_lamports()?             += amount;
                ctx.accounts.vault.pending_proposal = None;

                emit!(GuardianClaimEvent { vault: vault_key, destination: action_data, amount });
            }
        }
        Ok(())
    }
}

#[account]
pub struct Vault {
    pub owner:              Pubkey,
    pub duress_key:         Pubkey,
    pub guardians:          Vec<Pubkey>,
    pub guardian_threshold: u8,
    pub last_heartbeat:     i64,
    pub heartbeat_interval: i64,
    pub status:             VaultStatus,
    pub pending_withdrawal: Option<PendingWithdrawal>,
    pub pending_proposal:   Option<GuardianProposalData>,
    pub bump:               u8,
}

impl Vault {
    pub const SPACE: usize =
          8
        + 32
        + 32
        + (4 + 32 * MAX_GUARDIANS)
        + 1
        + 8
        + 8
        + 1
        + 1 + 8 + 32 + 8 + 8
        + 1 + 1 + 32 + (4 + 32 * MAX_GUARDIANS) + 8
        + 1
        + 64;
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, PartialEq, Eq, Default)]
pub enum VaultStatus {
    #[default]
    Active,
    Frozen,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct PendingWithdrawal {
    pub amount:            u64,
    pub destination:       Pubkey,
    pub initiated_at:      i64,
    pub timelock_duration: i64,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq)]
pub enum ProposalActionType {
    RotateOwner,
    GuardianClaim,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone)]
pub struct GuardianProposalData {
    pub action:      ProposalActionType,
    pub action_data: Pubkey,
    pub approvals:   Vec<Pubkey>,
    pub created_at:  i64,
}

#[derive(Accounts)]
pub struct InitializeVault<'info> {
    #[account(mut)]
    pub owner: Signer<'info>,
    #[account(
        init,
        payer = owner,
        space = Vault::SPACE,
        seeds = [b"vault", owner.key().as_ref()],
        bump,
    )]
    pub vault: Account<'info, Vault>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub depositor: Signer<'info>,
    #[account(
        mut,
        seeds = [b"vault", vault.owner.as_ref()],
        bump = vault.bump,
    )]
    pub vault: Account<'info, Vault>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct OwnerOnly<'info> {
    pub owner: Signer<'info>,
    #[account(
        mut,
        seeds = [b"vault", owner.key().as_ref()],
        bump = vault.bump,
        constraint = vault.owner == owner.key() @ SolGuardError::Unauthorized,
    )]
    pub vault: Account<'info, Vault>,
}

#[derive(Accounts)]
pub struct SignerOnVault<'info> {
    pub signer: Signer<'info>,
    #[account(
        mut,
        seeds = [b"vault", vault.owner.as_ref()],
        bump = vault.bump,
    )]
    pub vault: Account<'info, Vault>,
}

#[derive(Accounts)]
pub struct ExecuteWithdrawal<'info> {
    pub executor: Signer<'info>,
    #[account(
        mut,
        seeds = [b"vault", vault.owner.as_ref()],
        bump = vault.bump,
    )]
    pub vault: Account<'info, Vault>,
    /// CHECK: validated against pending_withdrawal.destination in instruction
    #[account(mut)]
    pub destination: AccountInfo<'info>,
}

#[derive(Accounts)]
pub struct ExecuteGuardianProposal<'info> {
    pub executor: Signer<'info>,
    #[account(
        mut,
        seeds = [b"vault", vault.owner.as_ref()],
        bump = vault.bump,
    )]
    pub vault: Account<'info, Vault>,
    /// CHECK: validated against proposal.action_data in instruction
    #[account(mut)]
    pub destination: AccountInfo<'info>,
}

#[event] pub struct VaultCreatedEvent        { pub vault: Pubkey, pub owner: Pubkey, pub guardian_count: u8 }
#[event] pub struct DepositEvent             { pub vault: Pubkey, pub amount: u64 }
#[event] pub struct HeartbeatEvent           { pub vault: Pubkey, pub owner: Pubkey, pub timestamp: i64 }
#[event] pub struct WithdrawalInitiatedEvent { pub vault: Pubkey, pub amount: u64, pub destination: Pubkey, pub unlock_at: i64, pub is_duress: bool }
#[event] pub struct WithdrawalExecutedEvent  { pub vault: Pubkey, pub amount: u64, pub destination: Pubkey }
#[event] pub struct WithdrawalCancelledEvent { pub vault: Pubkey, pub cancelled_by: Pubkey }
#[event] pub struct VaultFrozenEvent         { pub vault: Pubkey, pub frozen_by: Pubkey }
#[event] pub struct VaultUnfrozenEvent       { pub vault: Pubkey }
#[event] pub struct ProposalCreatedEvent     { pub vault: Pubkey, pub proposer: Pubkey, pub action: ProposalActionType }
#[event] pub struct ProposalApprovedEvent    { pub vault: Pubkey, pub approver: Pubkey, pub total_approvals: u8 }
#[event] pub struct OwnerRotatedEvent        { pub vault: Pubkey, pub new_owner: Pubkey }
#[event] pub struct GuardianClaimEvent       { pub vault: Pubkey, pub destination: Pubkey, pub amount: u64 }

#[error_code]
pub enum SolGuardError {
    #[msg("Vault is currently frozen")]                           VaultFrozen,
    #[msg("Vault must be frozen for this operation")]             VaultMustBeFrozen,
    #[msg("Unauthorized signer")]                                 Unauthorized,
    #[msg("Signer is not a guardian of this vault")]              NotAGuardian,
    #[msg("Guardian count must be between 1 and 5")]              InvalidGuardianCount,
    #[msg("Threshold must be >= 1 and <= guardian count")]        InvalidThreshold,
    #[msg("Duplicate guardian address")]                          DuplicateGuardian,
    #[msg("Heartbeat interval must be > 0")]                      InvalidHeartbeatInterval,
    #[msg("Amount must be > 0")]                                  InvalidAmount,
    #[msg("Insufficient vault balance")]                          InsufficientFunds,
    #[msg("Cannot withdraw 100% of balance at once")]             CannotWithdrawFullBalance,
    #[msg("A withdrawal is already pending")]                     WithdrawalAlreadyPending,
    #[msg("No pending withdrawal")]                               NoPendingWithdrawal,
    #[msg("Timelock has not expired yet")]                        TimelockNotExpired,
    #[msg("Wrong destination address")]                           WrongDestination,
    #[msg("No pending guardian proposal")]                        NoProposal,
    #[msg("Guardian has already approved this proposal")]         AlreadyApproved,
    #[msg("Not enough guardian approvals")]                       InsufficientApprovals,
    #[msg("Owner is still active (heartbeat not expired)")]       OwnerStillActive,
}
