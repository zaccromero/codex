//! Native owner handoff for one pending command. The destination is the original
//! approval waiter, which already owns the operation and selected executor.
//!
//! An embedding UI claims a review, presents its immutable details to the owner,
//! and hands the approved value to its in-process consumer for delivery. Taking
//! the review reserves the positive decision. Existing denial and cancellation
//! paths can still close the waiter. A retry registers a different attempt.
//!
//! Exec-server authenticates connections but has no receiver that ties a human
//! decision to its own pending operation. This handoff ends at the core waiter.
//! Cross-process delivery would require that receiver and a trusted binding to
//! the executor connection, including its cancellation and reconnect lifetime.

use super::PendingApproval;
use super::PendingApprovals;
use super::TurnState;
use crate::environment_selection::EnvironmentAttachment;
use codex_protocol::approvals::ExecApprovalKind;
use codex_protocol::models::AdditionalPermissionProfile;
use codex_protocol::models::SandboxPermissions;
use codex_protocol::protocol::ReviewDecision;
use codex_utils_path_uri::PathUri;
use std::sync::Arc;
use std::sync::PoisonError;
use std::sync::Weak;
use tokio_util::sync::CancellationToken;

/// Snapshot presented to the native owner. The prompt ID distinguishes retries
/// even when every displayed command field is identical.
#[derive(Clone)]
pub struct HumanCommandDetails {
    pub approval_id: String,
    pub turn_id: String,
    pub call_id: String,
    pub environment_id: String,
    pub command: Vec<String>,
    pub cwd: PathUri,
    pub sandbox_permissions: SandboxPermissions,
    pub additional_permissions: Option<AdditionalPermissionProfile>,
    pub tty: bool,
    pub reason: Option<String>,
}

pub(crate) struct HumanCommandAttempt {
    pub(crate) details: HumanCommandDetails,
    pub(crate) cancelled: CancellationToken,
    // The lifetime of the actual executor attachment captured by the runtime.
    pub(crate) executor: Weak<EnvironmentAttachment>,
}

impl HumanCommandAttempt {
    fn is_live(&self) -> bool {
        !self.cancelled.is_cancelled()
            && self
                .executor
                .upgrade()
                .is_some_and(|executor| !executor.detached.is_cancelled())
    }
}

/// Single-use native review handle. Dropping an unresolved review declines the attempt.
pub struct HumanCommandReview {
    attempt: Arc<HumanCommandAttempt>,
    approvals: Weak<PendingApprovals>,
}

impl HumanCommandReview {
    pub fn details(&self) -> &HumanCommandDetails {
        &self.attempt.details
    }

    /// Call only after the owner explicitly accepts this displayed attempt.
    pub fn approve_once(self) -> ApprovedHumanCommand {
        ApprovedHumanCommand(self)
    }

    fn take_waiter(&mut self) -> Option<PendingApproval> {
        let approvals = std::mem::take(&mut self.approvals).upgrade()?;
        let mut pending = approvals.lock().unwrap_or_else(PoisonError::into_inner);
        let key = &self.attempt.details.approval_id;
        let registered = pending.get(key)?.native_attempt.as_ref()?;
        if !Arc::ptr_eq(registered, &self.attempt) {
            return None;
        }
        pending.remove(key)
    }
}

impl Drop for HumanCommandReview {
    fn drop(&mut self) {
        if let Some(waiter) = self.take_waiter() {
            waiter.respond(ReviewDecision::denied("native human review closed"));
        }
    }
}

/// A native owner's decision addressed to its original pending attempt.
#[must_use]
pub struct ApprovedHumanCommand(HumanCommandReview);

impl ApprovedHumanCommand {
    /// Deliver once to the original approval waiter. A competing resolution,
    /// cancellation or executor detach invalidates an outstanding handoff.
    /// The waiter still applies its current policy and sandbox checks.
    pub fn deliver(mut self) -> bool {
        let Some(waiter) = self.0.take_waiter() else {
            return false;
        };
        if !self.0.attempt.is_live() {
            waiter.respond(ReviewDecision::Abort);
            return false;
        }
        waiter.sender.send(ApprovalReply::Native(self)).is_ok()
    }
}

/// Only the native owner handle can produce the second variant. The original
/// waiter rechecks its lifetime after waking, including time spent in the channel.
pub(crate) enum ApprovalReply {
    Decision(ReviewDecision),
    Native(ApprovedHumanCommand),
}

impl ApprovalReply {
    pub(crate) fn into_decision(self) -> ReviewDecision {
        match self {
            Self::Decision(decision) => decision,
            Self::Native(approved) if approved.0.attempt.is_live() => ReviewDecision::Approved,
            Self::Native(_) => ReviewDecision::Abort,
        }
    }
}

impl TurnState {
    pub(crate) fn take_human_command_review(
        &self,
        turn_id: &str,
        approval_id: &str,
    ) -> Option<HumanCommandReview> {
        let mut pending = self
            .pending_approvals
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let entry = pending.get_mut(approval_id)?;
        let attempt = entry.native_attempt.as_ref()?;
        if entry.kind != ExecApprovalKind::HumanCommand
            || entry.turn_id != turn_id
            || attempt.details.turn_id != turn_id
            || attempt.details.approval_id != approval_id
            || entry.sender.is_closed()
            || !attempt.is_live()
            || entry.native_review_taken
        {
            return None;
        }
        entry.native_review_taken = true;
        Some(HumanCommandReview {
            attempt: Arc::clone(attempt),
            approvals: Arc::downgrade(&self.pending_approvals),
        })
    }
}
