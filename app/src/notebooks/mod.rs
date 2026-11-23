mod context_menu;
pub mod editor;
pub mod file;
pub mod link;
mod styles;
pub mod telemetry;

use serde::{Deserialize, Serialize};
use warpui::AppContext;

use crate::server::ids::{ServerId, SyncId};

/// Identifier used by older saved notebook references.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct NotebookId(ServerId);
crate::server_id_traits! { NotebookId, "Notebook" }

impl From<NotebookId> for SyncId {
    fn from(id: NotebookId) -> Self {
        Self::ServerId(id.into())
    }
}

/// A notebook location. Mainly, this lets us distinguish between cloud and file-based notebooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum NotebookLocation {
    /// A cloud notebook in the user's personal space.
    PersonalCloud,
    /// A cloud notebook in a team space.
    Team,
    /// A notebook backed by a local file.
    LocalFile,
    /// A notebook backed by a remote file.
    RemoteFile,
}

/// Initialize notebooks-related keybindings.
pub fn init(app: &mut AppContext) {
    self::file::init(app);
    self::editor::view::init(app);
}
