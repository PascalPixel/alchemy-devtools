//! Alchemy's own layout: where its linker scripts live and which objects
//! each edition links, where its listings, scaffolds and drafts lived, what a
//! built edition leaves under out/, its twelve editions and their reference
//! ROMs, the overlays and the packed resource streams Camelot's
//! loader unpacks, the PO message catalogs, what counts as DONE, how a
//! measurement is published, and the humanization report.

pub(crate) mod built;
pub(crate) mod done;
pub(crate) mod humanity;
pub(crate) mod layout;
pub(crate) mod link;
pub(crate) mod lz;
pub(crate) mod overlay;
pub(crate) mod po;
pub(crate) mod publish;
pub(crate) mod targets;
