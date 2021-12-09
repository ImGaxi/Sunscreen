#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]

//! This crate contains the frontend compiler for Sunscreen circuits and the types and
//! algorithms that support it.

mod compiler;
mod error;
mod params;
/**
 * Types you can use as inputs and return values in your circuit.
 */
pub mod types;

use std::cell::RefCell;

use petgraph::{
    algo::is_isomorphic_matching,
    stable_graph::{NodeIndex, StableGraph},
    Graph,
};
use serde::{Deserialize, Serialize};

use sunscreen_backend::compile_inplace;
use sunscreen_circuit::{
    Circuit, EdgeInfo, Literal as CircuitLiteral, NodeInfo, Operation as CircuitOperation,
    OuterLiteral as CircuitOuterLiteral,
};

pub use compiler::Compiler;
pub use error::{Error, Result};
pub use params::PlainModulusConstraint;
pub use sunscreen_circuit::{SchemeType, SecurityLevel};
pub use sunscreen_runtime::Params;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
/**
 * Represents a literal node's data.
 */
pub enum Literal {
    /**
     * An unsigned 64-bit integer.
     */
    U64(u64),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
/**
 * Represents an operation occurring in the frontend AST.
 */
pub enum Operation {
    /**
     * This node indicates loading a cipher text from an input.
     */
    InputCiphertext,

    /**
     * Addition.
     */
    Add,

    /**
     * Multiplication.
     */
    Multiply,

    /**
     * A literal that serves as an operand to other operations.
     */
    Literal(Literal),

    /**
     * Rotate left.
     */
    RotateLeft,

    /**
     * Rotate right.
     */
    RotateRight,

    /**
     * In the BFV scheme, swap rows in the SIMD vectors.
     */
    SwapRows,

    /**
     * This node indicates the previous node's result should be a result of the circuit.
     */
    Output,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
/**
 * Information about an edge in the frontend IR.
 */
pub enum OperandInfo {
    /**
     * This edge serves as the left operand to the destination node.
     */
    Left,

    /**
     * This edge serves as the right operand to the destination node.
     */
    Right,

    /**
     * This edge serves as the single operand to the destination node.
     */
    Unary,
}

/**
 * This trait specifies a type as being able to be used as an input or output of a circuit.
 */
pub trait Value {
    /**
     * Creates an instance and adds it to the graph in the thread-local IR context.
     */
    fn new() -> Self;

    /**
     * Add a output node to the current IR context.
     */
    fn output(&self) -> Self;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
/**
 * Contains the frontend compilation graph.
 */
pub struct FrontendCompilation {
    /**
     * The dependency graph of the frontend's intermediate representation (IR) that backs a circuit.
     */
    pub graph: StableGraph<Operation, OperandInfo>,
}

#[derive(Clone, Debug)]
/**
 * The context under which std::ops:* on Value trait implementors should insert new nodes.
 */
pub struct Context {
    /**
     * The frontend compilation result.
     */
    pub compilation: FrontendCompilation,

    /**
     * The set of parameters for which we're currently constructing the graph.
     */
    pub params: Params,
}

impl PartialEq for FrontendCompilation {
    fn eq(&self, b: &Self) -> bool {
        is_isomorphic_matching(
            &Graph::from(self.graph.clone()),
            &Graph::from(b.graph.clone()),
            |n1, n2| n1 == n2,
            |e1, e2| e1 == e2,
        )
    }
}

thread_local! {
    /**
     * While constructing a circuit, this refers to the current intermediate representation.
     */
    pub static CURRENT_CTX: RefCell<Option<&'static mut Context>> = RefCell::new(None);
}

/**
 * Runs the specified closure, injecting the current circuit context.
 */
pub fn with_ctx<F, R>(f: F) -> R
where
    F: FnOnce(&mut Context) -> R,
{
    CURRENT_CTX.with(|ctx| {
        let mut option = ctx.borrow_mut();
        let ctx = option
            .as_mut()
            .expect("Called Ciphertext::new() outside of a context.");

        f(ctx)
    })
}

impl Context {
    /**
     * Creates a new empty frontend intermediate representation context with the given scheme.
     */
    pub fn new(params: &Params) -> Self {
        Self {
            compilation: FrontendCompilation {
                graph: StableGraph::new(),
            },
            params: params.clone(),
        }
    }

    fn add_2_input(&mut self, op: Operation, left: NodeIndex, right: NodeIndex) -> NodeIndex {
        let new_id = self.compilation.graph.add_node(op);
        self.compilation
            .graph
            .add_edge(left, new_id, OperandInfo::Left);
        self.compilation
            .graph
            .add_edge(right, new_id, OperandInfo::Right);

        new_id
    }

    fn add_1_input(&mut self, op: Operation, i: NodeIndex) -> NodeIndex {
        let new_id = self.compilation.graph.add_node(op);
        self.compilation
            .graph
            .add_edge(i, new_id, OperandInfo::Unary);

        new_id
    }

    /**
     * Add an input this context.
     */
    pub fn add_input(&mut self) -> NodeIndex {
        self.compilation.graph.add_node(Operation::InputCiphertext)
    }

    /**
     * Add an addition this context.
     */
    pub fn add_addition(&mut self, left: NodeIndex, right: NodeIndex) -> NodeIndex {
        self.add_2_input(Operation::Add, left, right)
    }

    /**
     * Add a multiplication this context.
     */
    pub fn add_multiplication(&mut self, left: NodeIndex, right: NodeIndex) -> NodeIndex {
        self.add_2_input(Operation::Multiply, left, right)
    }

    /**
     * Adds a literal to this context.
     */
    pub fn add_literal(&mut self, literal: Literal) -> NodeIndex {
        // See if we already have a node for the given literal. If so, just return it.
        // If not, make a new one.
        let existing_literal = self
            .compilation
            .graph
            .node_indices()
            .filter_map(|i| match &self.compilation.graph[i] {
                Operation::Literal(x) => {
                    if *x == literal {
                        Some(i)
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .nth(0);

        match existing_literal {
            Some(x) => x,
            None => self.compilation.graph.add_node(Operation::Literal(literal)),
        }
    }

    /**
     * Add a rotate left.
     */
    pub fn add_rotate_left(&mut self, left: NodeIndex, right: NodeIndex) -> NodeIndex {
        self.add_2_input(Operation::RotateLeft, left, right)
    }

    /**
     * Add a rotate right.
     */
    pub fn add_rotate_right(&mut self, left: NodeIndex, right: NodeIndex) -> NodeIndex {
        self.add_2_input(Operation::RotateRight, left, right)
    }

    /**
     * Add a node that captures the previous node as an output.
     */
    pub fn add_output(&mut self, i: NodeIndex) -> NodeIndex {
        self.add_1_input(Operation::Output, i)
    }
}

impl FrontendCompilation {
    /**
     * Performs frontend compilation of this intermediate representation into a backend [`Circuit`],
     * then perform backend compilation and return the result.
     */
    pub fn compile(&self) -> Circuit {
        let mut circuit = Circuit::new(SchemeType::Bfv);

        let mapped_graph = self.graph.map(
            |id, n| match n {
                Operation::Add => NodeInfo::new(CircuitOperation::Add),
                Operation::InputCiphertext => {
                    // HACKHACK: Input nodes are always added first to the graph in the order
                    // they're specified as function arguments. We should not depend on this.
                    NodeInfo::new(CircuitOperation::InputCiphertext(id.index()))
                }
                Operation::Literal(Literal::U64(x)) => NodeInfo::new(CircuitOperation::Literal(
                    CircuitOuterLiteral::Scalar(CircuitLiteral::U64(*x)),
                )),
                Operation::Multiply => NodeInfo::new(CircuitOperation::Multiply),
                Operation::Output => NodeInfo::new(CircuitOperation::OutputCiphertext),
                Operation::RotateLeft => NodeInfo::new(CircuitOperation::ShiftLeft),
                Operation::RotateRight => NodeInfo::new(CircuitOperation::ShiftRight),
                Operation::SwapRows => NodeInfo::new(CircuitOperation::SwapRows),
            },
            |_, e| match e {
                OperandInfo::Left => EdgeInfo::LeftOperand,
                OperandInfo::Right => EdgeInfo::RightOperand,
                OperandInfo::Unary => EdgeInfo::UnaryOperand,
            },
        );

        circuit.graph = StableGraph::from(mapped_graph);

        compile_inplace(circuit)
    }
}
