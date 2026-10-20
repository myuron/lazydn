use crate::action::{Action, Effect, Failure};
use crate::api::client::{ApiClient, RateLimit};
use crate::api::error::{ApiError, ErrorKind};
use crate::app::{LogTab, ProfileTab};
use crate::feed::{FeedKey, FeedPage};
use crate::model::User;

type Outcome = Result<(Action, Option<RateLimit>), ApiError>;

fn log_name(tab: LogTab) -> &'static str {
    match tab {
        LogTab::Following => "following",
        LogTab::Latest => "latest",
        LogTab::Trending => "trending",
    }
}

/// Calls the API for `effect` and returns what happened, in order: the `*Loaded` or `*Failed`
/// action first, then `ApiStatus` when the request reached the network.
/// Effects that need no API call return nothing. Call it from a spawned task per effect.
pub async fn execute(client: &ApiClient, effect: Effect) -> Vec<Action> {
    let Some(outcome) = call(client, &effect).await else {
        return Vec::new();
    };
    match outcome {
        Ok((action, rate)) => vec![action, status(rate, true)],
        Err(error) => {
            let blocked = match &error.kind {
                ErrorKind::Http { blocked_user, .. } => blocked_user.clone(),
                _ => None,
            };
            let mut out: Vec<Action> = failed(&effect, failure_of(&error), blocked)
                .into_iter()
                .collect();
            // A request refused locally never left the machine, so it says nothing about the link.
            match error.kind {
                ErrorKind::Network(_) => out.push(status(None, false)),
                ErrorKind::Http { .. } | ErrorKind::Decode(_) => {
                    out.push(status(None, true));
                }
                ErrorKind::SignInRequired
                | ErrorKind::InvalidArgument(_)
                | ErrorKind::InvalidBaseUrl
                | ErrorKind::Sign(_) => {}
            }
            out
        }
    }
}

pub(super) fn status(rate: Option<RateLimit>, online: bool) -> Action {
    Action::ApiStatus { rate, online }
}

pub(super) fn failure_of(error: &ApiError) -> Failure {
    Failure {
        code: error.code(),
        retry_after: match &error.kind {
            ErrorKind::Http { retry_after, .. } => *retry_after,
            _ => None,
        },
        message: match &error.kind {
            ErrorKind::Http { message, .. } => Some(message.clone()),
            _ => None,
        },
    }
}

async fn call(client: &ApiClient, effect: &Effect) -> Option<Outcome> {
    let outcome: Outcome = match effect {
        Effect::Quit
        | Effect::UseGuestClient
        | Effect::UseSignedClient
        | Effect::PrepareSignIn { .. }
        | Effect::OpenUrl(_)
        | Effect::CopyToClipboard(_)
        | Effect::RegisterDevice { .. }
        | Effect::SaveDrafts { .. }
        | Effect::LoadDrafts { .. }
        | Effect::CopyLink(_) => return None,
        Effect::DeleteCommit { id, req } => client.delete_commit(id).await.map(|r| {
            let action = Action::DeleteDone {
                id: id.clone(),
                req: *req,
            };
            (action, r.rate)
        }),
        Effect::ReportCommit {
            id,
            reason,
            details,
            req,
        } => client
            .report(id, *reason, details.as_deref())
            .await
            .map(|r| {
                let action = Action::ReportDone {
                    id: id.clone(),
                    req: *req,
                };
                (action, r.rate)
            }),
        Effect::StarCommit { id, on, req } => {
            let answer = if *on {
                client.star(id).await
            } else {
                client.unstar(id).await
            };
            answer.map(|r| {
                let action = Action::StarDone {
                    id: id.clone(),
                    req: *req,
                    commit: Box::new(r.data),
                };
                (action, r.rate)
            })
        }
        Effect::BookmarkCommit { id, on, req } => {
            let answer = if *on {
                client.bookmark(id).await
            } else {
                client.unbookmark(id).await
            };
            answer.map(|r| {
                let action = Action::BookmarkDone {
                    id: id.clone(),
                    req: *req,
                    commit: Box::new(r.data),
                };
                (action, r.rate)
            })
        }
        Effect::GivePastel { id, req } => client.give_pastel(id).await.map(|r| {
            let action = Action::PastelDone {
                id: id.clone(),
                req: *req,
                response: Box::new(r.data),
            };
            (action, r.rate)
        }),
        Effect::CreateCommit {
            body,
            parent_id,
            quoted_commit_id,
            req,
        } => client
            .create_commit(body, parent_id.as_deref(), quoted_commit_id.as_deref())
            .await
            .map(|r| {
                let action = Action::CommitCreated {
                    req: *req,
                    commit: Box::new(r.data),
                };
                (action, r.rate)
            }),
        Effect::FetchLog { tab, cursor, req } => client
            .log(log_name(*tab), None, cursor.as_deref())
            .await
            .map(|r| {
                let action = Action::FeedLoaded {
                    key: FeedKey::Log(*tab),
                    req: *req,
                    page: FeedPage::from(r.data),
                };
                (action, r.rate)
            }),
        Effect::FetchSaved { cursor, req } => client.saved(cursor.as_deref()).await.map(|r| {
            let action = Action::FeedLoaded {
                key: FeedKey::Saved,
                req: *req,
                page: FeedPage::from(r.data),
            };
            (action, r.rate)
        }),
        Effect::PollLog { tab, since } => {
            client
                .log(log_name(*tab), Some(since), None)
                .await
                .map(|r| {
                    let action = Action::NewCommits {
                        key: FeedKey::Log(*tab),
                        since: since.clone(),
                        count: r.data.new_commits_count.unwrap_or(0),
                    };
                    (action, r.rate)
                })
        }
        Effect::FetchThread { id, req } => client.commit(id).await.map(|r| {
            let action = Action::ThreadLoaded {
                id: id.clone(),
                req: *req,
                response: Box::new(r.data),
            };
            (action, r.rate)
        }),
        Effect::FetchReplies {
            thread,
            parent,
            cursor,
            req,
        } => client.replies(parent, cursor.as_deref()).await.map(|r| {
            let action = Action::RepliesLoaded {
                thread: thread.clone(),
                parent: parent.clone(),
                req: *req,
                page: r.data,
            };
            (action, r.rate)
        }),
        Effect::FetchUser { username, req } => client.user(username).await.map(|r| {
            let action = Action::UserLoaded {
                username: username.clone(),
                req: *req,
                response: Box::new(r.data),
            };
            (action, r.rate)
        }),
        Effect::FetchUserCommits {
            username,
            replies,
            cursor,
            req,
        } => client
            .user_commits(username, *replies, cursor.as_deref())
            .await
            .map(|r| {
                let action = Action::ProfileFeedLoaded {
                    username: username.clone(),
                    tab: profile_tab(*replies),
                    req: *req,
                    page: FeedPage::from(r.data),
                };
                (action, r.rate)
            }),
        Effect::FetchConnections {
            username,
            tab,
            cursor,
            req,
        } => {
            let page = match tab {
                crate::app::ConnTab::Followers => {
                    client.followers(username, cursor.as_deref()).await
                }
                crate::app::ConnTab::Following => {
                    client.following(username, cursor.as_deref()).await
                }
            };
            page.map(|r| {
                let action = Action::ConnectionsLoaded {
                    username: username.clone(),
                    tab: *tab,
                    req: *req,
                    page: r.data,
                };
                (action, r.rate)
            })
        }
        Effect::FetchMe { req } => client.me().await.map(|r| {
            let action = Action::MeLoaded {
                req: *req,
                response: Box::new(r.data),
            };
            (action, r.rate)
        }),
        Effect::FetchNotifications { types, cursor, req } => client
            .notifications(types, cursor.as_deref())
            .await
            .map(|r| {
                let action = Action::NotificationsLoaded {
                    req: *req,
                    page: Box::new(r.data),
                };
                (action, r.rate)
            }),
        Effect::FetchNotificationGroup {
            id,
            types,
            cursor,
            req,
        } => client
            .notification_group(id, types, cursor.as_deref())
            .await
            .map(|r| {
                let action = Action::GroupLoaded {
                    id: id.clone(),
                    req: *req,
                    page: r.data,
                };
                (action, r.rate)
            }),
        Effect::FetchMuted { cursor, req } => client.muted(cursor.as_deref()).await.map(|r| {
            let action = Action::MutedLoaded {
                req: *req,
                page: r.data,
            };
            (action, r.rate)
        }),
        Effect::FetchBlocked { cursor, req } => client.blocked(cursor.as_deref()).await.map(|r| {
            let action = Action::BlockedLoaded {
                req: *req,
                page: r.data,
            };
            (action, r.rate)
        }),
    };
    Some(outcome)
}

fn profile_tab(replies: bool) -> ProfileTab {
    if replies {
        ProfileTab::Replies
    } else {
        ProfileTab::Commits
    }
}

/// The `*Failed` action of `effect`; `PollLog` and effects without an API call have none.
fn failed(effect: &Effect, failure: Failure, blocked_user: Option<Box<User>>) -> Option<Action> {
    Some(match effect {
        Effect::FetchLog { tab, req, .. } => Action::FeedFailed {
            key: FeedKey::Log(*tab),
            req: *req,
            failure,
        },
        Effect::FetchSaved { req, .. } => Action::FeedFailed {
            key: FeedKey::Saved,
            req: *req,
            failure,
        },
        Effect::FetchThread { id, req } => Action::ThreadFailed {
            id: id.clone(),
            req: *req,
            failure,
        },
        Effect::FetchReplies {
            thread,
            parent,
            req,
            ..
        } => Action::RepliesFailed {
            thread: thread.clone(),
            parent: parent.clone(),
            req: *req,
            failure,
        },
        Effect::FetchUser { username, req } => Action::UserFailed {
            username: username.clone(),
            req: *req,
            failure,
            blocked_user,
        },
        Effect::FetchUserCommits {
            username,
            replies,
            req,
            ..
        } => Action::ProfileFeedFailed {
            username: username.clone(),
            tab: profile_tab(*replies),
            req: *req,
            failure,
        },
        Effect::FetchConnections {
            username, tab, req, ..
        } => Action::ConnectionsFailed {
            username: username.clone(),
            tab: *tab,
            req: *req,
            failure,
        },
        Effect::FetchMe { req } => Action::MeFailed { req: *req, failure },
        Effect::FetchNotifications { req, .. } => {
            Action::NotificationsFailed { req: *req, failure }
        }
        Effect::FetchNotificationGroup { id, req, .. } => Action::GroupFailed {
            id: id.clone(),
            req: *req,
            failure,
        },
        Effect::FetchMuted { req, .. } => Action::MutedFailed { req: *req, failure },
        Effect::FetchBlocked { req, .. } => Action::BlockedFailed { req: *req, failure },
        Effect::CreateCommit { req, .. } => Action::CommitFailed { req: *req, failure },
        Effect::PollLog { .. }
        | Effect::Quit
        | Effect::UseGuestClient
        | Effect::UseSignedClient
        | Effect::PrepareSignIn { .. }
        | Effect::OpenUrl(_)
        | Effect::CopyToClipboard(_)
        | Effect::RegisterDevice { .. }
        | Effect::SaveDrafts { .. }
        | Effect::LoadDrafts { .. }
        | Effect::CopyLink(_) => return None,
        Effect::DeleteCommit { id, req } => Action::DeleteFailed {
            id: id.clone(),
            req: *req,
            failure,
        },
        Effect::ReportCommit { id, req, .. } => Action::ReportFailed {
            id: id.clone(),
            req: *req,
            failure,
        },
        Effect::StarCommit { id, on, req } => Action::StarFailed {
            id: id.clone(),
            on: *on,
            req: *req,
            failure,
        },
        Effect::BookmarkCommit { id, on, req } => Action::BookmarkFailed {
            id: id.clone(),
            on: *on,
            req: *req,
            failure,
        },
        Effect::GivePastel { id, req } => Action::PastelFailed {
            id: id.clone(),
            req: *req,
            failure,
        },
    })
}

/// The `*Failed` action for `effect`, used when its task panicked and no response exists.
/// Returns `None` for effects that have no failure action, such as `PollLog`.
pub fn failure_for(effect: &Effect, code: &str) -> Option<Action> {
    failed(effect, Failure::new(code), None)
}
