//! Exercises the native embedding API against a live core approval waiter.

use super::*;
use crate::environment_selection::EnvironmentAttachment;
use crate::session::tests::make_session_and_context_with_rx;
use crate::session::tests::make_thread_for_session;
use crate::state::ActiveTurn;
use crate::state::TurnState;
use anyhow::Context as _;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use pretty_assertions::assert_eq;
use tokio::sync::Mutex;
use tokio_util::sync::DropGuard;
use tokio_util::task::AbortOnDropHandle;

#[derive(Clone, Copy)]
enum Target {
    Executor,
    Missing,
    Different,
    Child,
    Local,
    Ordinary,
}

struct PendingPrompt {
    thread: crate::CodexThread,
    state: Arc<Mutex<TurnState>>,
    turn_id: String,
    id: String,
    cancel: CancellationToken,
    attachment: Arc<EnvironmentAttachment>,
    detach: DropGuard,
    result: AbortOnDropHandle<ApprovalResult>,
}

impl PendingPrompt {
    async fn take(&self) -> Option<crate::HumanCommandReview> {
        self.thread
            .take_human_command_review(&self.turn_id, &self.id)
            .await
    }
}

async fn prompt(target: Target) -> anyhow::Result<PendingPrompt> {
    let (session, mut turn, events) = make_session_and_context_with_rx().await;
    if matches!(target, Target::Child) {
        let child = SessionSource::SubAgent(SubAgentSource::Other("child".into()));
        Arc::get_mut(&mut turn).unwrap().session_source = child;
    }
    let (thread, _submissions) = make_thread_for_session(
        Arc::clone(&session),
        &turn,
        events.clone(),
        turn.session_source.clone(),
    )
    .await;
    let active = ActiveTurn::default();
    let state = Arc::clone(&active.turn_state);
    *session.active_turn.lock().await = Some(active);
    let mut executor = turn.initial_environments.primary().unwrap().clone();
    executor.selection.environment_id = "executor".into();
    let url = (!matches!(target, Target::Local)).then(|| "ws://127.0.0.1:1".to_string());
    let (attachment, detach, _) = EnvironmentAttachment::new(
        &executor.selection.environment_id,
        Arc::new(codex_exec_server::Environment::create_for_tests(url)?),
    );
    executor.attachment = Arc::clone(&attachment);
    let cancel = CancellationToken::new();
    let mut review_context = GuardianReviewContext::from(&turn);
    review_context.approval_policy = AskForApproval::OnRequest;
    review_context.approvals_reviewer = ApprovalsReviewer::User;
    let ctx = ApprovalContext {
        exec_requirement: (!matches!(target, Target::Ordinary))
            .then_some(ExecApprovalRequirement::NeedsHumanApproval { reason: None }),
        review_context,
        cancellation_token: Some(cancel.clone()),
        call_id: "call".into(),
        tool_name: ToolName::plain("exec_command"),
        strict_auto_review: false,
        approval_reason: None,
        retry_reason: None,
        network_approval_context: None,
    };
    let action = ApprovalAction::ExecCommand {
        id: ctx.call_id.clone(),
        environment_id: if matches!(target, Target::Different) {
            "different".into()
        } else {
            executor.selection.environment_id.clone()
        },
        command: vec!["echo".into(), "two words".into()],
        hook_command: "echo 'two words'".into(),
        cwd: executor.cwd().clone(),
        sandbox_permissions: SandboxPermissions::UseDefault,
        additional_permissions: None,
        justification: None,
        tty: true,
        proposed_execpolicy_amendment: None,
    };
    let mut result = AbortOnDropHandle::new(tokio::spawn(async move {
        let binding = (!matches!(target, Target::Missing)).then_some(&executor);
        session
            .request_approval_for_attempt(action, ctx, binding)
            .await
    }));
    let request = tokio::select! {
        _ = &mut result => anyhow::bail!("expected pending human approval"),
        event = events.recv() => {
            let EventMsg::ExecApprovalRequest(request) = event?.msg else {
                anyhow::bail!("expected command approval");
            };
            request
        }
    };
    let id = if matches!(target, Target::Ordinary) {
        assert_eq!(request.kind, ExecApprovalKind::Command);
        request.effective_approval_id()
    } else {
        assert_eq!(request.kind, ExecApprovalKind::HumanCommand);
        request.approval_id.context("missing attempt id")?
    };
    Ok(PendingPrompt {
        thread,
        state,
        turn_id: turn.sub_id.clone(),
        id,
        cancel,
        attachment,
        detach,
        result,
    })
}

#[tokio::test]
async fn native_human_command_delivers_only_to_original_waiter() -> anyhow::Result<()> {
    let prompt = prompt(Target::Executor).await?;
    assert!(
        prompt
            .thread
            .take_human_command_review("other", &prompt.id)
            .await
            .is_none()
    );
    let review = prompt.take().await.context("native owner review")?;
    assert!(prompt.take().await.is_none());
    assert_eq!(review.details().command, vec!["echo", "two words"]);
    assert_eq!(review.details().environment_id, "executor");
    assert_eq!(review.details().approval_id, prompt.id);
    assert!(review.details().tty);
    assert!(review.approve_once().deliver());
    assert!(prompt.take().await.is_none());
    assert_eq!(
        prompt.result.await?.result.unwrap(),
        ReviewDecision::Approved
    );
    Ok(())
}

#[test_case::test_case(Target::Local; "local_executor")]
#[test_case::test_case(Target::Ordinary; "ordinary_approval")]
#[test_case::test_case(Target::Missing; "no_runtime_binding")]
#[test_case::test_case(Target::Different; "different_executor")]
#[test_case::test_case(Target::Child; "child_turn")]
#[tokio::test]
async fn native_human_command_requires_owning_executor(target: Target) -> anyhow::Result<()> {
    let prompt = prompt(target).await?;
    assert!(prompt.take().await.is_none());
    prompt
        .thread
        .session
        .notify_approval(&prompt.id, ReviewDecision::Abort)
        .await;
    assert!(prompt.result.await?.result.is_err());
    Ok(())
}

#[test_case::test_case(ReviewDecision::Approved; "wire_accept")]
#[test_case::test_case(ReviewDecision::ApprovedForSession; "session_accept")]
#[test_case::test_case(ReviewDecision::denied("denied"); "denial")]
#[test_case::test_case(ReviewDecision::Abort; "abort")]
#[tokio::test]
async fn native_human_command_competing_resolution(decision: ReviewDecision) -> anyhow::Result<()> {
    let prompt = prompt(Target::Executor).await?;
    let approved = prompt.take().await.unwrap().approve_once();
    prompt
        .thread
        .session
        .resolve_exec_approval(prompt.id, Some(prompt.turn_id), decision)
        .await;
    assert!(!approved.deliver());
    assert!(prompt.result.await?.result.is_err());
    Ok(())
}

#[tokio::test]
async fn native_human_command_abandoned_handoff_declines() -> anyhow::Result<()> {
    let prompt = prompt(Target::Executor).await?;
    drop(prompt.take().await.unwrap().approve_once());
    assert!(matches!(
        prompt.result.await?.result,
        Err(ToolError::Rejected(_))
    ));
    Ok(())
}

#[test_case::test_case(true; "turn_cancelled")]
#[test_case::test_case(false; "executor_detached")]
#[tokio::test]
async fn native_human_command_invalidated_before_delivery(cancel_turn: bool) -> anyhow::Result<()> {
    let prompt = prompt(Target::Executor).await?;
    let approved = prompt.take().await.unwrap().approve_once();
    if cancel_turn {
        prompt.cancel.cancel();
    } else {
        drop(prompt.detach);
    }
    assert!(!approved.deliver());
    assert!(prompt.result.await?.result.is_err());
    Ok(())
}

#[tokio::test]
async fn native_human_command_closed_waiter() -> anyhow::Result<()> {
    let prompt = prompt(Target::Executor).await?;
    let approved = prompt.take().await.unwrap().approve_once();
    prompt.result.abort();
    assert!(prompt.result.await.is_err_and(|error| error.is_cancelled()));
    assert!(!approved.deliver());
    Ok(())
}

#[tokio::test]
async fn native_human_command_cannot_claim_replacement_with_reused_id() -> anyhow::Result<()> {
    let prompt = prompt(Target::Executor).await?;
    let review = prompt.take().await.unwrap();
    let replacement = HumanCommandAttempt {
        details: review.details().clone(),
        cancelled: prompt.cancel.clone(),
        executor: Arc::downgrade(&prompt.attachment),
    };
    let approved = review.approve_once();
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    let old = prompt.state.lock().await.insert_pending_approval(
        prompt.id.clone(),
        tx,
        ExecApprovalKind::HumanCommand,
        prompt.turn_id.clone(),
        Some(replacement),
    );
    drop(old);
    assert!(!approved.deliver());
    assert!(matches!(
        rx.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ));
    assert!(prompt.take().await.unwrap().approve_once().deliver());
    let reply = rx.await?;
    drop(prompt.detach);
    assert_eq!(reply.into_decision(), ReviewDecision::Abort);
    assert!(prompt.result.await?.result.is_err());
    Ok(())
}
