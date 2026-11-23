use std::collections::{HashMap, HashSet};

use super::comment::{
    AttachedReviewComment, AttachedReviewCommentTarget, CommentId, CommentOrigin,
};
use super::pending_imported::{PendingImportedReviewComment, PendingImportedReviewCommentTarget};
use crate::code::buffer_location::LocalOrRemotePath;

/// Converts pending imported provider comments into attached review comments by:
/// * flattening threaded replies
/// * formatting markdown bodies
/// * converting repo-relative file paths to absolute file paths
pub(crate) fn attach_pending_imported_comments(
    pending_comments: Vec<PendingImportedReviewComment>,
    repo_path: &LocalOrRemotePath,
) -> Vec<AttachedReviewComment> {
    if pending_comments.is_empty() {
        return Vec::new();
    }

    group_review_comment_threads(&pending_comments)
        .into_iter()
        .map(|thread| {
            if let Some(missing_parent_id) = thread.missing_parent_id() {
                log::warn!(
                    "Importing orphaned comment (ID {:?}) with parent ID {:?}",
                    thread.root().github_comment_id(),
                    missing_parent_id
                );
            }
            attach_pending_imported_thread(thread, repo_path)
        })
        .collect()
}

fn attach_pending_imported_thread(
    thread: ReviewCommentThread<'_>,
    repo_path: &LocalOrRemotePath,
) -> AttachedReviewComment {
    let root = thread.root();
    let last_update_time = thread
        .comments()
        .iter()
        .map(|comment| comment.last_update_time)
        .max()
        .unwrap_or(root.last_update_time);

    let target = match &root.target {
        PendingImportedReviewCommentTarget::Line {
            relative_file_path,
            line,
            diff_content,
        } => AttachedReviewCommentTarget::Line {
            absolute_file_path: repo_path.join(&relative_file_path.to_string_lossy()),
            line: line.clone(),
            content: diff_content.clone(),
        },
        PendingImportedReviewCommentTarget::File { relative_file_path } => {
            AttachedReviewCommentTarget::File {
                absolute_file_path: repo_path.join(&relative_file_path.to_string_lossy()),
            }
        }
        PendingImportedReviewCommentTarget::General => AttachedReviewCommentTarget::General,
    };

    let origin = CommentOrigin::ImportedFromGitHub(root.github_details_without_parent());

    AttachedReviewComment {
        id: CommentId::new(),
        content: format_review_comment_thread(&thread),
        target,
        last_update_time,
        base: None,
        head: None,
        outdated: false,
        origin,
    }
}

const THREAD_REPLY_DIVIDER: &str = "\n---\n";

/// One root review comment and its replies in display order.
#[derive(Debug, PartialEq, Eq)]
struct ReviewCommentThread<'a> {
    comments: Vec<&'a PendingImportedReviewComment>,
    missing_parent_id: Option<&'a str>,
}

impl<'a> ReviewCommentThread<'a> {
    fn root(&self) -> &'a PendingImportedReviewComment {
        self.comments[0]
    }

    fn comments(&self) -> &[&'a PendingImportedReviewComment] {
        &self.comments
    }

    fn missing_parent_id(&self) -> Option<&'a str> {
        self.missing_parent_id
    }
}

/// Groups review comments into deterministic, depth-first provider threads.
///
/// Roots are ordered by comment ID. Replies are stably ordered by their
/// last-modified value. A comment whose parent is absent is retained as a root
/// and exposes that missing parent ID.
fn group_review_comment_threads(
    comments: &[PendingImportedReviewComment],
) -> Vec<ReviewCommentThread<'_>> {
    if comments.is_empty() {
        return Vec::new();
    }

    let existing_ids: HashSet<&str> = comments
        .iter()
        .map(PendingImportedReviewComment::github_comment_id)
        .collect();
    let mut roots: HashMap<&str, (&PendingImportedReviewComment, Option<&str>)> = HashMap::new();
    let mut parent_to_children: HashMap<&str, Vec<&PendingImportedReviewComment>> = HashMap::new();

    for comment in comments {
        match comment.github_parent_comment_id() {
            Some(parent_id) if existing_ids.contains(parent_id) => {
                parent_to_children
                    .entry(parent_id)
                    .or_default()
                    .push(comment);
            }
            missing_parent_id => {
                roots.insert(comment.github_comment_id(), (comment, missing_parent_id));
            }
        }
    }

    let mut roots: Vec<_> = roots.into_values().collect();
    roots.sort_by(|(a, _), (b, _)| a.github_comment_id().cmp(b.github_comment_id()));
    roots
        .into_iter()
        .map(|(root, missing_parent_id)| {
            let mut thread_comments = Vec::new();
            collect_review_comment_thread(root, &parent_to_children, &mut thread_comments);
            ReviewCommentThread {
                comments: thread_comments,
                missing_parent_id,
            }
        })
        .collect()
}

/// Formats a provider thread in review-comment Markdown.
fn format_review_comment_thread(thread: &ReviewCommentThread<'_>) -> String {
    thread
        .comments()
        .iter()
        .map(|comment| format!("**@{}**:\n{}", comment.author(), comment.body))
        .collect::<Vec<_>>()
        .join(THREAD_REPLY_DIVIDER)
}

fn collect_review_comment_thread<'a>(
    comment: &'a PendingImportedReviewComment,
    children_map: &HashMap<&str, Vec<&'a PendingImportedReviewComment>>,
    result: &mut Vec<&'a PendingImportedReviewComment>,
) {
    result.push(comment);

    if let Some(children) = children_map.get(comment.github_comment_id()) {
        let mut sorted_children = children.to_vec();
        sorted_children.sort_by_key(|comment| comment.last_update_time);
        for child in sorted_children {
            collect_review_comment_thread(child, children_map, result);
        }
    }
}
