//! Data export (Phase 3 workstream B): the local dataset as CSVs + JSON
//! arrays, zipped or as a plain directory. User-facing data dumps — kept
//! separate from `backup` (VACUUM INTO snapshot), which stays the recovery
//! path.

pub mod csv;
