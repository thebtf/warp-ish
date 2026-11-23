mod batch;
mod comment;
#[cfg(test)]
mod diff_hunk_parser;
mod flatten;
mod pending_imported;

pub(crate) use batch::{ReviewCommentBatch, ReviewCommentBatchEvent};
#[cfg(test)]
pub(crate) use comment::ImportedCommentDetails;
pub(crate) use comment::{
    AttachedReviewComment, AttachedReviewCommentTarget, CommentId, CommentOrigin, LineDiffContent,
};
pub(crate) use flatten::attach_pending_imported_comments;
pub(crate) use pending_imported::PendingImportedReviewComment;
#[cfg(test)]
pub(crate) use pending_imported::PendingImportedReviewCommentTarget;
