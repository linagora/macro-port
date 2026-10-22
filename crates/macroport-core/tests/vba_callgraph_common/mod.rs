// SPDX-License-Identifier: Apache-2.0
//! What the tests of the call graph share: a module read the way the front end
//! reads it, and fed to the graph a procedure at a time.

// clippy.toml excuses `expect` in the `#[test]` functions only, not in helpers.
#![allow(clippy::expect_used)]

use macroport_core::rules::ModuleType;
use macroport_core::vba::{
    CallGraph, ConstTable, Facts, ModuleId, Procedure, StatementReader, parse, split_lines,
};

/// A statement of a procedure: the position of its logical line among those of
/// the module, and its facts.
pub type Statement = (usize, Facts);

/// A module, parsed and read statement by statement.
pub struct Module {
    pub module_type: ModuleType,
    pub procedures: Vec<Procedure>,
    /// For each procedure, in order, the statements of its logical lines, its
    /// header and its end included.
    pub statements: Vec<Vec<Statement>>,
}

impl Module {
    /// Reads the module `text`. Its constants are the ones declared outside its
    /// procedures, which the statements may use.
    pub fn read(module_type: ModuleType, text: &str) -> Self {
        let lines = split_lines(text).expect("a text within the limits");
        let parsed = parse(&lines);
        let mut inside = vec![false; lines.logical.len()];
        for procedure in &parsed.procedures {
            for at in procedure.lines.clone() {
                if let Some(inside) = inside.get_mut(at) {
                    *inside = true;
                }
            }
        }
        let mut consts = ConstTable::default();
        for (line, _) in (lines.logical.iter().zip(&inside)).filter(|(_, inside)| !**inside) {
            consts.declare(line);
        }
        let mut reader = StatementReader::new(&consts);
        let statements = parsed.procedures.iter().map(|procedure| {
            let range = lines.logical.get(procedure.lines.clone());
            let range = range.expect("the lines of a procedure");
            let lines = (procedure.lines.clone()).zip(range);
            lines.map(|(at, line)| (at, reader.read(line))).collect()
        });
        let statements = statements.collect();
        Self {
            module_type,
            procedures: parsed.procedures,
            statements,
        }
    }

    /// Adds the module to the graph and reads its statements, a procedure at a
    /// time. `started[i]` tells that the host starts the procedure `i`.
    pub fn add_to(&self, graph: &mut CallGraph, started: &[bool]) -> ModuleId {
        let module = graph.add_module(self.module_type, &self.procedures, started);
        for (position, statements) in self.statements.iter().enumerate() {
            for (line, facts) in statements {
                graph.read(module, position, *line, facts);
            }
        }
        module
    }
}
