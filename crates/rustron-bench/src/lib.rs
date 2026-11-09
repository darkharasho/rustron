//! Bench statistics and parity-gate rules for comparing Rustron with Electron.
pub mod gate;
pub mod prepare;
pub mod proc_tree;
pub mod report_server;
pub mod stats;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Engine {
    Rustron,
    Electron,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeReport {
    pub engine: String,
    pub grid_since_launch: f64,
    pub grid_since_nav: f64,
    pub json_ms: f64,
    pub sort_ms: f64,
    pub loop_ms: f64,
    pub frame_ms: Vec<f64>,
    pub type_ms: Vec<f64>,
    pub errors: u32,
    pub fail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResult {
    pub engine: Engine,
    pub report: Option<ProbeReport>,
    pub mem_mb: Option<f64>,
    pub timed_out: bool,
}

pub use gate::{evaluate, render_markdown, summarize, GateRow, Summary};
