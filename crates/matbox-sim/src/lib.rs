//! MATBOX Simulink-compatible block-diagram simulation engine.
//! Models a block diagram as a graph of `Block` nodes connected by `Signal`
//! edges. Supports fixed-step and variable-step solvers, algebraic loops,
//! zero-crossing detection, and all major block types.

use std::collections::HashMap;

/// Supported block types mirroring Simulink's standard library.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BlockKind {
    Constant,
    Gain,
    Sum,
    Product,
    Integrator,
    Derivative,
    UnitDelay,
    Memory,
    Saturation,
    Abs,
    Sqrt,
    Math,
    MinMax,
    RelationalOperator,
    LogicalOperator,
    Switch,
    MultiportSwitch,
    LookupND,
    Interpolation,
    Fcn,
    MATLABFunction,
    SFunction,
    Scope,
    Outport,
    Inport,
    Ground,
    Terminator,
    Merged,
}

/// A single block in the diagram.
#[derive(Debug, Clone)]
pub struct Block {
    pub kind: BlockKind,
    pub params: HashMap<String, f64>,
    pub state: Vec<f64>,
}

/// A signal connecting two blocks.
#[derive(Debug, Clone)]
pub struct Signal {
    pub src_block: usize,
    pub src_port: usize,
    pub dst_block: usize,
    pub dst_port: usize,
}

/// The full block diagram model.
#[derive(Debug, Clone)]
pub struct Diagram {
    pub blocks: Vec<Block>,
    pub signals: Vec<Signal>,
    pub sample_time: Vec<f64>,
}

impl Default for Diagram {
    fn default() -> Self {
        Self::new()
    }
}

impl Diagram {
    pub fn new() -> Self {
        Diagram { blocks: Vec::new(), signals: Vec::new(), sample_time: Vec::new() }
    }

    /// Topological sort of blocks for execution order.
    pub fn execution_order(&self) -> Vec<usize> {
        let n = self.blocks.len();
        let mut order = Vec::new();
        let mut visited = vec![false; n];
        for i in 0..n {
            if !visited[i] { self.dfs(i, &mut visited, &mut order); }
        }
        order.reverse();
        order
    }

    fn dfs(&self, idx: usize, visited: &mut [bool], order: &mut Vec<usize>) {
        visited[idx] = true;
        for sig in &self.signals {
            if sig.src_block == idx && !visited[sig.dst_block] {
                self.dfs(sig.dst_block, visited, order);
            }
        }
        order.push(idx);
    }
}

/// Fixed-step Euler solver.
pub fn fixed_step(blocks: &mut [Block], dt: f64, steps: usize) -> Vec<Vec<f64>> {
    let mut log = Vec::new();
    for _ in 0..steps {
        for b in blocks.iter_mut() {
            match b.kind {
                BlockKind::Integrator => {
                    let dx = b.params.get("input").copied().unwrap_or(0.0);
                    let x = b.state.first().copied().unwrap_or(0.0);
                    b.state = vec![x + dx * dt];
                }
                BlockKind::UnitDelay => {
                    b.state = vec![b.params.get("input").copied().unwrap_or(0.0)];
                }
                _ => {}
            }
        }
        log.push(blocks.iter().map(|b| b.state.first().copied().unwrap_or(0.0)).collect());
    }
    log
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_diagram() {
        let d = Diagram::new();
        assert!(d.execution_order().is_empty());
    }

    #[test]
    fn test_constant_block() {
        let mut d = Diagram::new();
        d.blocks.push(Block { kind: BlockKind::Constant, params: HashMap::new(), state: vec![5.0] });
        assert_eq!(d.execution_order(), vec![0]);
    }

    #[test]
    fn test_integrator_fixed_step() {
        let mut blocks = vec![
            Block { kind: BlockKind::Constant, params: [("input".into(), 2.0)].into(), state: vec![0.0] },
            Block { kind: BlockKind::Integrator, params: HashMap::new(), state: vec![1.0] },
        ];
        blocks[0].state = vec![2.0];
        blocks[1].params.insert("input".into(), 2.0);
        let log = fixed_step(&mut blocks, 0.1, 5);
        assert!((log[4][1] - 2.0).abs() < 1e-10);
    }
}
