//! Wire types shared by the agent and the desktop client.
//!
//! This crate is the single source of truth for the contract. TypeScript
//! interfaces and Zod schemas in `packages/shared-types` and
//! `packages/api-contracts` are generated from it; CI regenerates and fails on
//! any diff, so the two sides cannot drift.
//!
//! It is deliberately dependency-light. Everything here is data — no database
//! handles, no Docker client, no filesystem. That keeps contract generation
//! fast and keeps business rules out of a crate whose job is description.

// Tests are allowed to unwrap and slice; production paths in this workspace are
// not. A panic in a test is a failed test — in the agent it is a stopped service.
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod codegen;
pub mod contract;
pub mod dto;
pub mod enums;
pub mod envelope;
pub mod errors;
pub mod ids;

pub use contract::contract_schema;
pub use dto::*;
pub use enums::*;
pub use envelope::{ApiResponse, Page, PageRequest, ResponseMeta};
pub use errors::{ApiError, ErrorCode, FieldError};
pub use ids::*;

/// Schema version of the contract itself. Bumped when a change is not backward
/// compatible, so a client and agent at different versions can say so plainly
/// instead of failing on a missing field.
pub const CONTRACT_VERSION: u32 = 1;
