//! MATBOX graphics: a serializable plot specification model.
//!
//! The engine emits a `PlotSpec`; the shared React/TS UI (WebGL/Canvas) and the
//! Tauri desktop renderer consume it. No drawing happens here.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LineSpecStyle {
    Solid,
    Dashed,
    Dotted,
    DashDot,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Series {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub z: Option<Vec<f64>>,
    pub color: Option<[f64; 3]>,
    pub line_style: LineSpecStyle,
    pub marker: Option<String>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Axis {
    pub xlabel: Option<String>,
    pub ylabel: Option<String>,
    pub zlabel: Option<String>,
    pub title: Option<String>,
    pub xlim: Option<[f64; 2]>,
    pub ylim: Option<[f64; 2]>,
    pub zlim: Option<[f64; 2]>,
    pub grid: bool,
    pub xscale: String, // "linear" | "log"
    pub yscale: String,
    pub view: Option<[f64; 2]>,
}

impl Default for Axis {
    fn default() -> Self {
        Axis {
            xlabel: None, ylabel: None, zlabel: None, title: None,
            xlim: None, ylim: None, zlim: None, grid: false,
            xscale: "linear".into(), yscale: "linear".into(), view: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PlotKind {
    Line2D,
    Line3D,
    Scatter,
    Surface,
    Bar,
    Histogram,
    Image,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlotSpec {
    pub kind: PlotKind,
    pub series: Vec<Series>,
    pub axis: Axis,
    pub colormap: Option<String>,
    pub cdata: Option<Vec<f64>>,
    pub cdata_dims: Option<[usize; 2]>,
}

impl PlotSpec {
    pub fn line2d() -> Self {
        PlotSpec {
            kind: PlotKind::Line2D,
            series: Vec::new(),
            axis: Axis::default(),
            colormap: None,
            cdata: None,
            cdata_dims: None,
        }
    }
}
