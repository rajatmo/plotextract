//! The use cases: what the program does, as calls on plain state, with no window, thread, disk or network. They depend on
//! the domain, the algorithms and the ports, and nothing outside reaches into them but through their public calls.
//!
//! - [`session`]: the work on a vector figure: calibration, series, selection and its undo.
//! - [`library`]: opening figures, projects and exports, through the [`Files`](plotextract_ports::Files) port and a
//!   [`Registry`] of formats.
//! - [`wizard`]: tracing an image in steps (crop and mask, colours, plot area, groups, links), each step owning its state.

pub mod colours;
pub mod library;
mod registry;
pub mod session;
pub mod wizard;

pub use library::{Opened, Source, Workspace, WorkspaceError};
pub use registry::Registry;
pub use session::{Session, SessionError};
