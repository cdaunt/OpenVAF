use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::io::{self, Write};

use hir::{BranchKind, CompilationDB};
use hir_lower::{CallBackKind, CurrentKind, HirInterner, ParamKind};
use lasso::Rodeo;
use mir::{Block, Function, Inst, InstructionData, Param, Value, ValueDef};

use crate::dae::ResidualNatureKind;
use crate::init::CacheSlot;
use crate::{CompiledModule, SimUnknownKind};

// BranchId registry keyed on (hi_name, lo_name) so both the dae.unknowns
// and the HirInterner params sections produce consistent ids for the same branch.
struct BranchReg {
    map: HashMap<(String, String), u32>,
    next: u32,
}

impl BranchReg {
    fn new() -> Self {
        Self { map: HashMap::new(), next: 0 }
    }

    fn named_id(&mut self, branch: hir::Branch, db: &CompilationDB) -> u32 {
        let (hi_s, lo_s) = branch_hi_lo(branch, db);
        let key = (hi_s, lo_s.unwrap_or_default());
        *self.map.entry(key).or_insert_with(|| { let id = self.next; self.next += 1; id })
    }

    fn unnamed_id(&mut self, hi: hir::Node, lo: Option<hir::Node>, db: &CompilationDB) -> u32 {
        let hi_s = hi.name(db).to_string();
        let lo_s = lo.map(|n| n.name(db).to_string()).unwrap_or_default();
        let key = (hi_s, lo_s);
        *self.map.entry(key).or_insert_with(|| { let id = self.next; self.next += 1; id })
    }
}

fn vname(v: Value) -> String { format!("{v}") }
fn bname(b: Block) -> String { format!("{b}") }
fn funcref_name(f: mir::FuncRef) -> String { format!("{f}") }

fn jstr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"'  => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => { let _ = write!(out, "\\u{:04x}", c as u32); }
            c => out.push(c),
        }
    }
    out
}

fn param_kind_json(
    kind: &ParamKind,
    idx: u32,
    db: &CompilationDB,
    branches: &mut BranchReg,
) -> String {
    match kind {
        ParamKind::Voltage { hi, lo } => {
            let hi_s = hi.name(db);
            let lo_s = lo.map(|n| n.name(db));
            let hi_node = hi_s.clone();
            let lo_node = lo_s.clone().unwrap_or_default();
            match lo_s {
                Some(lo_str) => format!(
                    r#"{{"tag":"Voltage","hi":"{hi_s}","lo":"{lo_str}","hi_node":"{hi_node}","lo_node":"{lo_node}"}}"#
                ),
                None => format!(
                    r#"{{"tag":"Voltage","hi":"{hi_s}","lo":null,"hi_node":"{hi_node}","lo_node":null}}"#
                ),
            }
        }
        ParamKind::Current(ck) => current_kind_json(ck, idx, db, branches),
        ParamKind::Param(param) => {
            let name = jstr(&param.name(db));
            format!(r#"{{"tag":"Param","name":"{name}","param_id":{idx}}}"#)
        }
        ParamKind::ParamGiven { param } => {
            let name = jstr(&param.name(db));
            format!(r#"{{"tag":"ParamGiven","name":"{name}","param_id":{idx}}}"#)
        }
        ParamKind::Temperature => r#"{"tag":"Temperature"}"#.to_string(),
        ParamKind::Abstime => r#"{"tag":"Abstime"}"#.to_string(),
        ParamKind::ParamSysFun(sf) => {
            let name = format!("{sf:?}");
            format!(r#"{{"tag":"ParamSysFun","name":"{name}"}}"#)
        }
        ParamKind::HiddenState(var) => {
            let name = jstr(&var.name(db));
            format!(r#"{{"tag":"HiddenState","var":"{name}","var_id":{idx}}}"#)
        }
        ParamKind::PortConnected { port } => {
            let name = jstr(&port.name(db));
            format!(r#"{{"tag":"PortConnected","port":"{name}"}}"#)
        }
        ParamKind::PrevState(state) => {
            let idx = usize::from(*state);
            format!(r#"{{"tag":"PrevState","index":{idx}}}"#)
        }
        ParamKind::NewState(state) => {
            let idx = usize::from(*state);
            format!(r#"{{"tag":"NewState","index":{idx}}}"#)
        }
        ParamKind::EnableLim => r#"{"tag":"EnableLim"}"#.to_string(),
        ParamKind::EnableIntegration => r#"{"tag":"EnableIntegration"}"#.to_string(),
        ParamKind::ImplicitUnknown(eq) => {
            let idx = usize::from(*eq);
            format!(r#"{{"tag":"ImplicitUnknown","id":{idx}}}"#)
        }
    }
}

fn current_kind_json(
    ck: &CurrentKind,
    idx: u32,
    db: &CompilationDB,
    branches: &mut BranchReg,
) -> String {
    match ck {
        CurrentKind::Branch(branch) => {
            let bid = branches.named_id(*branch, db);
            let branch_name = jstr(&branch.name(db));
            let (hi_s, lo_s) = branch_hi_lo(*branch, db);
            let lo_json = match &lo_s {
                Some(s) => format!(r#""{s}""#),
                None => "null".to_string(),
            };
            format!(
                r#"{{"tag":"CurrentBranch","branch":"{branch_name}","branch_id":{bid},"hi":"{hi_s}","lo":{lo_json}}}"#
            )
        }
        CurrentKind::Unnamed { hi, lo } => {
            let bid = branches.unnamed_id(*hi, *lo, db);
            let hi_s = jstr(&hi.name(db));
            let lo_json = match lo {
                Some(lo_node) => format!(r#""{}""#, jstr(&lo_node.name(db))),
                None => "null".to_string(),
            };
            format!(
                r#"{{"tag":"CurrentUnnamed","branch_id":{bid},"hi":"{hi_s}","lo":{lo_json}}}"#
            )
        }
        CurrentKind::Port(port) => {
            let name = jstr(&port.name(db));
            format!(r#"{{"tag":"CurrentPort","port":"{name}"}}"#)
        }
    }
}

fn branch_hi_lo(branch: hir::Branch, db: &CompilationDB) -> (String, Option<String>) {
    match branch.kind(db) {
        BranchKind::Nodes(hi, lo) => (hi.name(db).to_string(), Some(lo.name(db).to_string())),
        BranchKind::NodeGnd(hi) => (hi.name(db).to_string(), None),
        BranchKind::PortFlow(port) => (port.name(db).to_string(), None),
    }
}

fn callback_name(kind: &CallBackKind, db: &CompilationDB, _literals: &Rodeo) -> String {
    match kind {
        CallBackKind::TimeDerivative => "ddt".to_string(),
        CallBackKind::NodeDerivative(node) => format!("ddx_{}", node.name(db)),
        CallBackKind::Derivative(_) => "ddx_param".to_string(),
        CallBackKind::SimParam => "simparam".to_string(),
        CallBackKind::SimParamOpt => "SimParamOpt".to_string(),
        CallBackKind::SimParamStr => "simparam_str".to_string(),
        CallBackKind::Analysis => "Analysis".to_string(),
        CallBackKind::CollapseHint(hi, lo) => {
            let hi_s = hi.name(db);
            match lo {
                Some(lo_node) => format!("collapse_{hi_s}_Some({})", lo_node.name(db)),
                None => format!("collapse_{hi_s}_None"),
            }
        }
        CallBackKind::StoreLimit(_) => "StoreLimit".to_string(),
        CallBackKind::LimDiscontinuity => "LimDiscontinuity".to_string(),
        CallBackKind::BuiltinLimit { .. } => "BuiltinLimit".to_string(),
        CallBackKind::WhiteNoise { .. } => "WhiteNoise".to_string(),
        CallBackKind::FlickerNoise { .. } => "FlickerNoise".to_string(),
        CallBackKind::Print { .. } => "Print".to_string(),
        CallBackKind::ParamInfo(_, _) => "set_Invalid".to_string(),
        CallBackKind::SetRetFlag(_) => "set_Invalid".to_string(),
        CallBackKind::NoiseTable(_) => "WhiteNoise".to_string(),
    }
}

fn emit_function_json(
    fn_name: &str,
    func: &Function,
    intern: &HirInterner,
    db: &CompilationDB,
    literals: &Rodeo,
    cache_mapping: Option<&[(Value, CacheSlot)]>,
    branches: &mut BranchReg,
) -> String {
    let mut out = String::new();
    let _ = write!(out, "{{\n");
    let _ = write!(out, "    \"name\": \"{fn_name}\",\n");

    // Collect all function params in order (interner params + trailing cslot bridges).
    let mut param_vals: Vec<(Param, Value)> = func.dfg.values()
        .filter_map(|v| {
            if let ValueDef::Param(p) = func.dfg.value_def(v) { Some((p, v)) } else { None }
        })
        .collect();
    param_vals.sort_by_key(|(p, _)| usize::from(*p));
    let all_args: Vec<String> = param_vals.iter().map(|(_, v)| vname(*v)).collect();
    let _ = write!(out, "    \"args\": [");
    for (i, a) in all_args.iter().enumerate() {
        if i > 0 { let _ = write!(out, ", "); }
        let _ = write!(out, "\"{a}\"");
    }
    let _ = write!(out, "],\n");

    let _ = write!(out, "    \"params\": {{");
    let n = intern.params.raw.len();
    for (idx, (kind, val)) in intern.params.raw.iter().enumerate() {
        let ssa = vname(*val);
        let kind_json = param_kind_json(kind, idx as u32, db, branches);
        if idx > 0 { let _ = write!(out, ","); }
        let _ = write!(out, "\n      \"{ssa}\": {kind_json}");
    }
    if n > 0 { let _ = write!(out, "\n    "); }
    let _ = write!(out, "}},\n");

    let _ = emit_constants(func, literals, &mut out);

    let _ = write!(out, "    \"call_decls\": [");
    let cbs: Vec<_> = intern.callbacks.raw.iter().enumerate().collect();
    for (i, (idx, kind)) in cbs.iter().enumerate() {
        let inst_name = format!("inst{idx}");
        let cb = callback_name(kind, db, literals);
        if i > 0 { let _ = write!(out, ","); }
        let _ = write!(out, "\n      {{\"name\": \"{inst_name}\", \"raw\": \"fn %{cb}\"}}");
    }
    if !cbs.is_empty() { let _ = write!(out, "\n    "); }
    let _ = write!(out, "],\n");

    let _ = emit_blocks(func, literals, &mut out);

    let _ = write!(out, "    \"cache_mapping\": [");
    if let Some(mapping) = cache_mapping {
        for (i, (init_val, cslot)) in mapping.iter().enumerate() {
            let v = vname(*init_val);
            let cs = format!("{cslot}");
            if i > 0 { let _ = write!(out, ","); }
            let _ = write!(out, "\n      {{\"init_value\": \"{v}\", \"cslot\": \"{cs}\"}}");
        }
        if !mapping.is_empty() { let _ = write!(out, "\n    "); }
    }
    let _ = write!(out, "]\n");

    let _ = write!(out, "  }}");
    out
}

fn emit_constants(func: &Function, literals: &Rodeo, out: &mut String) {
    let mut fconsts: Vec<(String, f64)> = Vec::new();
    let mut iconsts: Vec<(String, i64)> = Vec::new();
    let mut bconsts: Vec<(String, bool)> = Vec::new();
    let mut sconsts: Vec<(String, String)> = Vec::new();

    for val in func.dfg.values() {
        match func.dfg.value_def(val) {
            ValueDef::Const(mir::Const::Float(f)) => fconsts.push((vname(val), f64::from(f))),
            ValueDef::Const(mir::Const::Int(i))   => iconsts.push((vname(val), i as i64)),
            ValueDef::Const(mir::Const::Bool(b))  => bconsts.push((vname(val), b)),
            ValueDef::Const(mir::Const::Str(s))   => sconsts.push((vname(val), jstr(&literals[s]))),
            _ => {}
        }
    }

    let _ = write!(out, "    \"constants\": {{");
    let total = fconsts.len() + iconsts.len() + bconsts.len() + sconsts.len();
    let mut written = 0;
    for (name, v) in &fconsts {
        if written > 0 { let _ = write!(out, ","); }
        // JSON has no inf/nan literals.
        let repr = if v.is_finite() {
            format!("{v}")
        } else if v.is_infinite() {
            if *v > 0.0 { r#""__inf__""#.to_string() } else { r#""__neginf__""#.to_string() }
        } else {
            r#""__nan__""#.to_string()
        };
        let _ = write!(out, "\n      \"{name}\": {repr}");
        written += 1;
    }
    for (name, v) in &iconsts {
        if written > 0 { let _ = write!(out, ","); }
        let _ = write!(out, "\n      \"{name}\": {v}");
        written += 1;
    }
    for (name, v) in &bconsts {
        if written > 0 { let _ = write!(out, ","); }
        let _ = write!(out, "\n      \"{name}\": {v}");
        written += 1;
    }
    for (name, v) in &sconsts {
        if written > 0 { let _ = write!(out, ","); }
        let _ = write!(out, "\n      \"{name}\": \"{v}\"");
        written += 1;
    }
    if total > 0 { let _ = write!(out, "\n    "); }
    let _ = write!(out, "}},\n");
}

fn emit_blocks(func: &Function, _literals: &Rodeo, out: &mut String) {
    let _ = write!(out, "    \"blocks\": [");
    let mut block_cursor = func.layout.blocks_cursor();
    let mut first_block = true;
    while let Some(bb) = block_cursor.next(&func.layout) {
        if !first_block { let _ = write!(out, ","); }
        first_block = false;
        let label = bname(bb);
        let _ = write!(out, "\n      {{\"label\": \"{label}\", \"insts\": [");
        let mut first_inst = true;
        for inst in func.layout.block_insts(bb) {
            if !first_inst { let _ = write!(out, ","); }
            first_inst = false;
            emit_inst(func, inst, out);
        }
        if !first_inst { let _ = write!(out, "\n      "); }
        let _ = write!(out, "]}}");
    }
    if !first_block { let _ = write!(out, "\n    "); }
    let _ = write!(out, "],\n");
}

fn emit_inst(func: &Function, inst: Inst, out: &mut String) {
    let result = func.dfg.inst_results(inst).first().copied().map(vname);
    let result_json = match &result {
        Some(r) => format!("\"{r}\""),
        None => "null".to_string(),
    };
    let _ = write!(out, "\n        {{\"result\": {result_json}, ");
    match &func.dfg.insts[inst] {
        InstructionData::Unary { opcode, arg } => {
            let _ = write!(out, "\"opcode\": \"{opcode}\", \"operands\": [\"{}\"]", vname(*arg));
        }
        InstructionData::Binary { opcode, args } => {
            let _ = write!(
                out,
                "\"opcode\": \"{opcode}\", \"operands\": [\"{}\", \"{}\"]",
                vname(args[0]), vname(args[1])
            );
        }
        InstructionData::Branch { cond, then_dst, else_dst, .. } => {
            let _ = write!(
                out,
                "\"opcode\": \"br\", \"condition\": \"{}\", \"true_block\": \"{}\", \"false_block\": \"{}\"",
                vname(*cond), bname(*then_dst), bname(*else_dst)
            );
        }
        InstructionData::Jump { destination } => {
            let _ = write!(out, "\"opcode\": \"jmp\", \"targets\": [\"{}\"]", bname(*destination));
        }
        InstructionData::Exit => {
            let _ = write!(out, "\"opcode\": \"exit\", \"operands\": []");
        }
        InstructionData::PhiNode(phi) => {
            let _ = write!(out, "\"opcode\": \"phi\", \"phi_edges\": [");
            let edges: Vec<_> = func.dfg.phi_edges(phi).collect();
            for (i, (bb, val)) in edges.iter().enumerate() {
                if i > 0 { let _ = write!(out, ", "); }
                let _ = write!(out, "{{\"value\": \"{}\", \"block\": \"{}\"}}", vname(*val), bname(*bb));
            }
            let _ = write!(out, "]");
        }
        InstructionData::Call { func_ref, .. } => {
            let fn_name = funcref_name(*func_ref);
            let arg_vals: Vec<_> = func.dfg.instr_args(inst)
                .iter()
                .map(|&v| format!("\"{}\"", vname(v)))
                .collect();
            let _ = write!(
                out,
                "\"opcode\": \"call\", \"call_target\": \"{fn_name}\", \"operands\": [{}]",
                arg_vals.join(", ")
            );
        }
    }
    let _ = write!(out, "}}");
}

fn emit_dae_json(module: &CompiledModule<'_>, db: &CompilationDB, branches: &mut BranchReg) -> String {
    let mut out = String::new();
    let dae = &module.dae_system;
    let _ = write!(out, "{{\n");

    let _ = write!(out, "    \"unknowns\": {{");
    for (idx, kind) in dae.unknowns.raw.iter().enumerate() {
        if idx > 0 { let _ = write!(out, ","); }
        let sim_key = format!("sim_node{idx}");
        let name = sim_unknown_name(kind, db, branches);
        let _ = write!(out, "\n      \"{sim_key}\": \"{name}\"");
    }
    if !dae.unknowns.raw.is_empty() { let _ = write!(out, "\n    "); }
    let _ = write!(out, "}},\n");

    let _ = write!(out, "    \"residual\": {{");
    for (idx, residual) in dae.residual.raw.iter().enumerate() {
        if idx > 0 { let _ = write!(out, ","); }
        let sim_key = format!("sim_node{idx}");
        let nature = match residual.nature_kind {
            ResidualNatureKind::Flow => "Flow",
            ResidualNatureKind::Potential => "Potential",
            ResidualNatureKind::Switch => "Switch",
        };
        let _ = write!(
            out,
            "\n      \"{sim_key}\": {{\"resist\": \"{}\", \"react\": \"{}\", \
             \"resist_small_signal\": \"{}\", \"react_small_signal\": \"{}\", \
             \"resist_lim_rhs\": \"{}\", \"react_lim_rhs\": \"{}\", \
             \"nature_kind\": \"{nature}\"}}",
            vname(residual.resist), vname(residual.react),
            vname(residual.resist_small_signal), vname(residual.react_small_signal),
            vname(residual.resist_lim_rhs), vname(residual.react_lim_rhs),
        );
    }
    if !dae.residual.raw.is_empty() { let _ = write!(out, "\n    "); }
    let _ = write!(out, "}},\n");

    let _ = write!(out, "    \"jacobian\": {{");
    for (i, entry) in dae.jacobian.raw.iter().enumerate() {
        if i > 0 { let _ = write!(out, ","); }
        let row = format!("{}", entry.row);
        let col = format!("{}", entry.col);
        let key = format!("{row},{col}");
        let _ = write!(
            out,
            "\n      \"{key}\": {{\"row\": \"{row}\", \"col\": \"{col}\", \
             \"resist\": \"{}\", \"react\": \"{}\"}}",
            vname(entry.resist), vname(entry.react),
        );
    }
    if !dae.jacobian.raw.is_empty() { let _ = write!(out, "\n    "); }
    let _ = write!(out, "}},\n");

    let _ = write!(out, "    \"num_resistive\": {},\n", dae.num_resistive);
    let _ = write!(out, "    \"num_reactive\": {}\n", dae.num_reactive);
    let _ = write!(out, "  }}");
    out
}

fn sim_unknown_name(kind: &SimUnknownKind, db: &CompilationDB, branches: &mut BranchReg) -> String {
    match kind {
        SimUnknownKind::KirchoffLaw(node) => node.name(db).to_string(),
        SimUnknownKind::Current(ck) => {
            let bid = match ck {
                CurrentKind::Branch(branch) => branches.named_id(*branch, db),
                CurrentKind::Unnamed { hi, lo } => branches.unnamed_id(*hi, *lo, db),
                CurrentKind::Port(port) => branches.unnamed_id(*port, None, db),
            };
            format!("br[Branch(BranchId({bid}))]")
        }
        SimUnknownKind::Implicit(eq) => format!("{eq}"),
    }
}

pub fn write_json<W: Write>(
    module: &CompiledModule<'_>,
    db: &CompilationDB,
    literals: &Rodeo,
    w: &mut W,
) -> io::Result<()> {
    let mut branches = BranchReg::new();
    for kind in module.dae_system.unknowns.raw.iter() {
        match kind {
            SimUnknownKind::Current(CurrentKind::Branch(br)) => { branches.named_id(*br, db); }
            SimUnknownKind::Current(CurrentKind::Unnamed { hi, lo }) => { branches.unnamed_id(*hi, *lo, db); }
            _ => {}
        }
    }

    let module_name = module.info.module.name(db);
    let ports: Vec<_> = module.info.module.ports(db);
    let internal: Vec<_> = module.info.module.internal_nodes(db);

    let w_str = |w: &mut W, s: &str| -> io::Result<()> { w.write_all(s.as_bytes()) };

    w_str(w, "{\n")?;
    w_str(w, &format!("  \"schema_version\": 1,\n"))?;
    w_str(w, &format!("  \"name\": \"{}\",\n", jstr(&module_name)))?;

    let port_strs: Vec<_> = ports.iter().map(|n| format!("\"{}\"", n.name(db))).collect();
    w_str(w, &format!("  \"ports\": [{}],\n", port_strs.join(", ")))?;
    w_str(w, &format!("  \"port_nodes\": [{}],\n", port_strs.join(", ")))?;

    let int_strs: Vec<_> = internal.iter().map(|n| format!("\"{}\"", n.name(db))).collect();
    w_str(w, &format!("  \"internal_nodes\": [{}],\n", int_strs.join(", ")))?;

    let cache_pairs: Vec<(Value, CacheSlot)> =
        module.init.cached_vals.iter().map(|(&v, &cs)| (v, cs)).collect();
    let eval_json = emit_function_json("", &module.eval, &module.intern, db, literals, None, &mut branches);
    w_str(w, &format!("  \"eval_fn\": {eval_json},\n"))?;

    let init_json = emit_function_json("_init", &module.init.func, &module.init.intern, db, literals, Some(&cache_pairs), &mut branches);
    w_str(w, &format!("  \"init_fn\": {init_json},\n"))?;

    let setup_json = emit_function_json("_setup", &module.model_param_setup, &module.model_param_intern, db, literals, None, &mut branches);
    w_str(w, &format!("  \"setup_fn\": {setup_json},\n"))?;

    let dae_json = emit_dae_json(module, db, &mut branches);
    w_str(w, &format!("  \"dae\": {dae_json},\n"))?;

    let mna_json = emit_mna_json(module, db, &branches);
    w_str(w, &format!("  \"mna\": {mna_json}\n"))?;

    w_str(w, "}\n")?;
    Ok(())
}

// Positional MNA view — mirrors OsdiDescriptor so MNA simulators (vajax, VACASK)
// can consume the JSON without needing to parse the dae section's sim_nodeN naming.
// nodes[i] == sim_node{i}, first num_terminals are the external ports.
fn emit_mna_json(module: &CompiledModule<'_>, db: &CompilationDB, branches: &BranchReg) -> String {
    let mut out = String::new();
    let dae = &module.dae_system;
    let n_terminals = module.info.module.ports(db).len();
    let n_nodes = dae.unknowns.raw.len();

    let _ = write!(out, "{{\n");
    let _ = write!(out, "    \"num_nodes\": {n_nodes},\n");
    let _ = write!(out, "    \"num_terminals\": {n_terminals},\n");

    let _ = write!(out, "    \"nodes\": [");
    for (idx, kind) in dae.unknowns.raw.iter().enumerate() {
        if idx > 0 { let _ = write!(out, ","); }
        let (name, node_kind) = mna_node_info(kind, db, branches);
        let _ = write!(out, "\n      {{\"idx\": {idx}, \"name\": \"{name}\", \"kind\": \"{node_kind}\"}}");
    }
    if n_nodes > 0 { let _ = write!(out, "\n    "); }
    let _ = write!(out, "],\n");

    let _ = write!(out, "    \"residuals\": [");
    for (idx, res) in dae.residual.raw.iter().enumerate() {
        if idx > 0 { let _ = write!(out, ","); }
        let _ = write!(
            out,
            "\n      {{\"node\": {idx}, \"resist\": \"{}\", \"react\": \"{}\"}}",
            vname(res.resist), vname(res.react),
        );
    }
    if !dae.residual.raw.is_empty() { let _ = write!(out, "\n    "); }
    let _ = write!(out, "],\n");

    let _ = write!(out, "    \"jacobian_entries\": [");
    for (i, entry) in dae.jacobian.raw.iter().enumerate() {
        if i > 0 { let _ = write!(out, ","); }
        let row = usize::from(entry.row);
        let col = usize::from(entry.col);
        let resist_ssa = vname(entry.resist);
        let react_ssa  = vname(entry.react);
        let has_resist = entry.resist != mir::F_ZERO;
        let has_react  = entry.react  != mir::F_ZERO;
        let _ = write!(
            out,
            "\n      {{\"row\": {row}, \"col\": {col}, \
             \"resist\": \"{resist_ssa}\", \"react\": \"{react_ssa}\", \
             \"has_resist\": {has_resist}, \"has_react\": {has_react}}}",
        );
    }
    if !dae.jacobian.raw.is_empty() { let _ = write!(out, "\n    "); }
    let _ = write!(out, "],\n");

    let _ = write!(out, "    \"num_noise_sources\": {},\n", dae.noise_sources.len());
    let _ = write!(out, "    \"num_jacobian_entries\": {},\n", dae.jacobian.raw.len());
    let _ = write!(out, "    \"num_resistive_jacobian_entries\": {},\n", dae.num_resistive);
    let _ = write!(out, "    \"num_reactive_jacobian_entries\": {}\n", dae.num_reactive);
    let _ = write!(out, "  }}");
    out
}

fn mna_node_info(
    kind: &SimUnknownKind,
    db: &CompilationDB,
    _branches: &BranchReg,
) -> (String, &'static str) {
    match kind {
        SimUnknownKind::KirchoffLaw(node) => (node.name(db).to_string(), "voltage"),
        SimUnknownKind::Current(ck) => {
            let name = match ck {
                CurrentKind::Branch(br) => {
                    let (hi, lo) = branch_hi_lo(*br, db);
                    match lo {
                        Some(lo) => format!("I({hi},{lo})"),
                        None     => format!("I({hi})"),
                    }
                }
                CurrentKind::Unnamed { hi, lo } => {
                    let hi_s = hi.name(db);
                    match lo {
                        Some(lo_node) => format!("I({hi_s},{})", lo_node.name(db)),
                        None          => format!("I({hi_s})"),
                    }
                }
                CurrentKind::Port(port) => format!("I<{}>", port.name(db)),
            };
            (name, "current")
        }
        SimUnknownKind::Implicit(eq) => (format!("{eq}"), "implicit"),
    }
}
