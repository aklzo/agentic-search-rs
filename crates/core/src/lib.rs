//! Core engine of the agentic web research tool.
//!
//! Frontends (CLI, GUI) wire together [`config::Config`], the provider
//! factories in [`llm`] / [`search`] / [`fetch`], and run
//! [`agent::ResearchAgent`]. Progress can be observed via [`events`].

pub mod agent;
pub mod config;
pub mod error;
pub mod events;
pub mod fetch;
pub mod llm;
pub mod search;
