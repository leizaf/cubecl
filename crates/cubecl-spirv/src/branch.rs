use cubecl_core::ir::Branch;
use cubecl_core::ir::{self as core, Select};
use cubecl_opt::{ControlFlow, NodeIndex};
use rspirv::{
    dr::Operand,
    spirv::{LoopControl, SelectionControl, Word},
};

use crate::{item::Item, variable::Variable, SpirvCompiler, SpirvTarget};

impl<T: SpirvTarget> SpirvCompiler<T> {
    pub fn compile_branch(&mut self, branch: Branch) {
        if let Branch::Select(Select {
            cond,
            then,
            or_else,
            out,
        }) = branch
        {
            self.compile_select(cond, then, or_else, out)
        }
    }

    pub fn compile_read_bound(
        &mut self,
        arr: &Variable,
        index: Word,
        item: Item,
        read: impl FnOnce(&mut Self) -> Word,
    ) -> Word {
        let ty = item.id(self);
        let len = self.length(arr, None);
        let bool = self.type_bool();
        let cond = self.u_less_than(bool, None, index, len).unwrap();

        let current_block = self.current_block.unwrap();

        let in_bounds = self.id();
        let fallback = self.id();
        let next = self.id();

        self.selection_merge(next, SelectionControl::DONT_FLATTEN)
            .unwrap();
        self.branch_conditional(cond, in_bounds, fallback, vec![1, 0])
            .unwrap();

        self.begin_block(Some(in_bounds)).unwrap();
        let value = read(self);
        self.branch(next).unwrap();

        self.begin_block(Some(fallback)).unwrap();
        let fallback_value = item.constant(self, 0u32.into());
        self.branch(next).unwrap();

        self.state.end_labels.insert(current_block, next);

        self.begin_block(Some(next)).unwrap();
        self.phi(
            ty,
            None,
            vec![(value, in_bounds), (fallback_value, fallback)],
        )
        .unwrap()
    }

    pub fn compile_write_bound(
        &mut self,
        arr: &Variable,
        index: Word,
        write: impl FnOnce(&mut Self),
    ) {
        let len = self.length(arr, None);
        let bool = self.type_bool();
        let cond = self.u_less_than(bool, None, index, len).unwrap();
        let current_block = self.current_block.unwrap();

        let in_bounds = self.id();
        let next = self.id();

        self.selection_merge(next, SelectionControl::DONT_FLATTEN)
            .unwrap();
        self.branch_conditional(cond, in_bounds, next, vec![1, 0])
            .unwrap();

        self.begin_block(Some(in_bounds)).unwrap();
        write(self);
        self.branch(next).unwrap();

        self.begin_block(Some(next)).unwrap();
        self.state.end_labels.insert(current_block, next);
    }

    pub fn compile_copy_bound(
        &mut self,
        input: &Variable,
        out: &Variable,
        in_index: Word,
        out_index: Word,
        len: Option<u32>,
        copy: impl FnOnce(&mut Self),
    ) {
        let in_len = self.length(input, None);
        let out_len = self.length(out, None);
        let bool = self.type_bool();
        let int = self.type_int(32, 0);
        let in_index = match len {
            Some(len) => self.i_add(int, None, in_index, len).unwrap(),
            None => in_index,
        };
        let out_index = match len {
            Some(len) => self.i_add(int, None, out_index, len).unwrap(),
            None => out_index,
        };
        let cond_in = self.u_less_than(bool, None, in_index, in_len).unwrap();
        let cond_out = self.u_less_than(bool, None, out_index, out_len).unwrap();
        let cond = self.logical_and(bool, None, cond_in, cond_out).unwrap();

        let current_block = self.current_block.unwrap();

        let in_bounds = self.id();
        let next = self.id();

        self.selection_merge(next, SelectionControl::DONT_FLATTEN)
            .unwrap();
        self.branch_conditional(cond, in_bounds, next, vec![1, 0])
            .unwrap();

        self.begin_block(Some(in_bounds)).unwrap();
        copy(self);
        self.branch(next).unwrap();

        self.begin_block(Some(next)).unwrap();
        self.state.end_labels.insert(current_block, next);
    }

    fn compile_select(
        &mut self,
        cond: core::Variable,
        then: core::Variable,
        or_else: core::Variable,
        out: core::Variable,
    ) {
        let cond = self.compile_variable(cond);
        let then = self.compile_variable(then);
        let or_else = self.compile_variable(or_else);
        let out = self.compile_variable(out);

        let then_ty = then.item();
        let ty = then_ty.id(self);

        let cond_id = self.read(&cond);
        let then = self.read(&then);
        let or_else = self.read_as(&or_else, &then_ty);
        let out_id = self.write_id(&out);

        self.select(ty, Some(out_id), cond_id, then, or_else)
            .unwrap();
        self.write(&out, out_id);
    }

    pub fn compile_control_flow(&mut self, control_flow: ControlFlow) {
        match control_flow {
            ControlFlow::Break {
                cond,
                body,
                or_break,
            } => self.compile_break(cond, body, or_break),
            ControlFlow::IfElse {
                cond,
                then,
                or_else,
                merge,
            } => self.compile_if_else(cond, then, or_else, merge),
            ControlFlow::Switch {
                value,
                default,
                branches,
                merge,
            } => self.compile_switch(value, default, branches, merge),
            ControlFlow::Loop {
                body,
                continue_target,
                merge,
            } => self.compile_loop(body, continue_target, merge),
            ControlFlow::Return => {
                self.ret().unwrap();
                self.current_block = None;
            }
            ControlFlow::None => {
                let opt = self.opt.clone();
                let children = opt.sucessors(self.current_block.unwrap());
                assert_eq!(
                    children.len(),
                    1,
                    "None control flow should have only 1 outgoing edge"
                );
                let label = self.label(children[0]);
                self.branch(label).unwrap();
                self.compile_block(children[0]);
            }
        }
    }

    fn compile_break(&mut self, cond: core::Variable, body: NodeIndex, or_break: NodeIndex) {
        let cond = self.compile_variable(cond);
        let body_label = self.label(body);
        let break_label = self.label(or_break);
        let cond_id = self.read(&cond);

        self.branch_conditional(cond_id, body_label, break_label, None)
            .unwrap();
        self.compile_block(body);
        self.compile_block(or_break);
    }

    fn compile_if_else(
        &mut self,
        cond: core::Variable,
        then: NodeIndex,
        or_else: NodeIndex,
        merge: NodeIndex,
    ) {
        let cond = self.compile_variable(cond);
        let then_label = self.label(then);
        let else_label = self.label(or_else);
        let merge_label = self.label(merge);
        let cond_id = self.read(&cond);

        self.selection_merge(merge_label, SelectionControl::NONE)
            .unwrap();
        self.branch_conditional(cond_id, then_label, else_label, None)
            .unwrap();
        self.compile_block(then);
        self.compile_block(or_else);
        self.compile_block(merge);
    }

    fn compile_switch(
        &mut self,
        value: core::Variable,
        default: NodeIndex,
        branches: Vec<(u32, NodeIndex)>,
        merge: NodeIndex,
    ) {
        let value = self.compile_variable(value);
        let value_id = self.read(&value);

        let default_label = self.label(default);
        let targets = branches
            .iter()
            .map(|(value, block)| {
                let label = self.label(*block);
                (Operand::LiteralBit32(*value), label)
            })
            .collect::<Vec<_>>();
        let merge_label = self.label(merge);

        self.selection_merge(merge_label, SelectionControl::NONE)
            .unwrap();
        self.switch(value_id, default_label, targets).unwrap();
        self.compile_block(default);
        for (_, block) in branches {
            self.compile_block(block);
        }
        self.compile_block(merge);
    }

    fn compile_loop(&mut self, body: NodeIndex, continue_target: NodeIndex, merge: NodeIndex) {
        let body_label = self.label(body);
        let continue_label = self.label(continue_target);
        let merge_label = self.label(merge);

        self.loop_merge(merge_label, continue_label, LoopControl::NONE, vec![])
            .unwrap();
        self.branch(body_label).unwrap();
        self.compile_block(body);
        self.compile_block(continue_target);
        self.compile_block(merge);
    }
}