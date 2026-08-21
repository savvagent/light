//! Engine trait seams: providers, tools, workspaces, and the permission gate. No I/O.

pub mod tool;
pub mod traits;
pub mod types;

pub use tool::{Decision, NeverPause, PauseController, PermissionGate, Tool};
pub use traits::{Provider, Workspace, WorkspaceRead};
pub use types::{CompleteRequest, CompleteResponse, Edit, Usage};
