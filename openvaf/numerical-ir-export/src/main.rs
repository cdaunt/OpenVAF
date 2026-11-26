//! Offline, data-only numerical interchange exporter. No target-language codegen.
use basedb::diagnostics::ConsoleSink;
use hir::{CompilationDB, Parameter, Type};
use hir_lower::{HirInterner, ParamKind, PlaceKind};
use mir::{Const, Function, InstructionData, Opcode, Value, ValueDef};
use paths::AbsPathBuf;
use serde_json::{json, Value as Json};
use sha2::{Digest, Sha256};
use sim_back::{collect_modules, CompiledModule, SimUnknownKind};
use std::{collections::HashMap, path::Path};

type Result<T> = std::result::Result<T, String>;
fn id(v: Value) -> usize {
    usize::from(v)
}

fn operation(op: Opcode) -> Result<&'static str> {
    use Opcode::*;
    Ok(match op {
        Fadd => "add",
        Fsub => "subtract",
        Fmul => "multiply",
        Fdiv => "divide",
        Fneg => "negate",
        OptBarrier => "identity",
        Flt => "less",
        Fgt => "greater",
        Fle => "less_equal",
        Fge => "greater_equal",
        Feq => "equal",
        Fne => "not_equal",
        Bnot => "not",
        Sqrt => "sqrt",
        Exp => "exp",
        Ln => "ln",
        Log => "log10",
        Sin => "sin",
        Cos => "cos",
        Tan => "tan",
        Asin => "asin",
        Acos => "acos",
        Atan => "atan",
        Sinh => "sinh",
        Cosh => "cosh",
        Tanh => "tanh",
        Asinh => "asinh",
        Acosh => "acosh",
        Atanh => "atanh",
        Floor => "floor",
        Ceil => "ceil",
        Pow => "pow",
        Hypot => "hypot",
        Atan2 => "atan2",
        _ => return Err(format!("unsupported numerical operation: {op}")),
    })
}

fn function(f: &Function, inputs: &HashMap<usize, Json>, outputs: &[Value]) -> Result<Json> {
    if !f.validate() {
        return Err("invalid producer MIR".into());
    }
    let mut values = Vec::new();
    for v in f.dfg.values() {
        let source = match f.dfg.value_def(v) {
            ValueDef::Const(Const::Float(n)) => {
                Some(json!({"kind":"real","bits":format!("{:016x}",f64::from(n).to_bits())}))
            }
            ValueDef::Const(Const::Bool(value)) => Some(json!({"kind":"boolean","value":value})),
            ValueDef::Const(Const::Int(value)) => Some(json!({"kind":"integer","value":value})),
            ValueDef::Param(p) if !f.dfg.value_dead(v) => Some(
                inputs
                    .get(&usize::from(p))
                    .ok_or_else(|| format!("unbound MIR input {p}"))?
                    .clone(),
            ),
            _ => None,
        };
        if let Some(source) = source {
            values.push(json!({"id":id(v),"source":source}));
        }
    }
    let entry = f.layout.blocks().next().ok_or("empty MIR")?;
    let mut blocks = Vec::new();
    for b in f.layout.blocks() {
        let mut phis = Vec::new();
        let mut instructions = Vec::new();
        let mut terminator = json!({"kind":"return"});
        let mut terminated = false;
        for inst in f.layout.block_insts(b) {
            if terminated {
                return Err("instructions after terminator".into());
            }
            let results = f.dfg.inst_results(inst);
            match &f.dfg.insts[inst] {
                InstructionData::PhiNode(phi)=>{
                    if !instructions.is_empty(){return Err("phi after ordinary instruction".into());}
                    let incoming=phi.edges(&f.dfg.insts.value_lists,&f.dfg.phi_forest).map(|(b,v)|json!({"predecessor":usize::from(b),"value":id(v)})).collect::<Vec<_>>();
                    phis.push(json!({"result":id(results[0]),"incoming":incoming}));
                }
                InstructionData::Unary{opcode,arg}=>instructions.push(json!({"result":id(results[0]),"op":operation(*opcode)?,"args":[id(*arg)]})),
                InstructionData::Binary{opcode,args}=>instructions.push(json!({"result":id(results[0]),"op":operation(*opcode)?,"args":[id(args[0]),id(args[1])]})),
                InstructionData::Jump{destination}=>{terminator=json!({"kind":"jump","target":usize::from(*destination)});terminated=true;},
                InstructionData::Branch{cond,then_dst,else_dst,..}=>{terminator=json!({"kind":"branch","condition":id(*cond),"then_block":usize::from(*then_dst),"else_block":usize::from(*else_dst)});terminated=true;},
                InstructionData::Exit=>{terminated=true;},
                InstructionData::Call{func_ref,..}=>return Err(format!("unsupported callback: {}",f.dfg.signatures[*func_ref].name)),
            }
        }
        // OpenVAF MIR represents the final fallthrough block as an implicit
        // function exit. Normalize that convention only; other blocks must end
        // explicitly rather than accidentally becoming extra return sites.
        if !terminated && Some(b) != f.layout.blocks().last() {
            return Err("unterminated non-exit MIR block".into());
        }
        blocks.push(json!({"id":usize::from(b),"phis":phis,"instructions":instructions,"terminator":terminator}));
    }
    Ok(
        json!({"entry":usize::from(entry),"values":values,"blocks":blocks,"outputs":outputs.iter().map(|v|id(*v)).collect::<Vec<_>>()}),
    )
}

fn inputs(
    f: &Function,
    intern: &HirInterner,
    module: &CompiledModule<'_>,
    params: &[Parameter],
) -> Result<HashMap<usize, Json>> {
    let mut out = HashMap::new();
    for (i, (kind, v)) in intern.params.raw.iter().enumerate() {
        if f.dfg.value_dead(*v) {
            continue;
        }
        let node =
            |n| module.dae_system.unknowns.index(&SimUnknownKind::KirchoffLaw(n)).map(usize::from);
        let source = match *kind {
            ParamKind::Param(p) => {
                json!({"kind":"parameter","index":params.iter().position(|v|*v==p).ok_or("unknown parameter")?})
            }
            ParamKind::Temperature => json!({"kind":"parameter","index":params.len()}),
            ParamKind::ParamGiven { .. } => json!({"kind":"boolean","value":true}),
            ParamKind::ParamSysFun(hir::ParamSysFun::mfactor) => {
                json!({"kind":"real","bits":"3ff0000000000000"})
            }
            ParamKind::Voltage { hi, lo } => {
                json!({"kind":"unknown_difference","positive":node(hi),"negative":lo.and_then(node)})
            }
            _ => return Err(format!("unsupported model input: {kind:?}")),
        };
        out.insert(i, source);
    }
    Ok(out)
}

fn export(input: &Path, output: &Path) -> Result<()> {
    let source = std::fs::read(input).map_err(|e| e.to_string())?;
    let input = AbsPathBuf::assert(input.canonicalize().map_err(|e| e.to_string())?);
    let db = CompilationDB::new_fs(input, &[], &[], &[]).map_err(|e| e.to_string())?;
    let modules =
        collect_modules(&db, false, &mut ConsoleSink::new(&db)).ok_or("Verilog-A diagnostics")?;
    if modules.len() != 1 {
        return Err("exactly one module required".into());
    }
    let info = &modules[0];
    let module = CompiledModule::new(&db, info, &mut lasso::Rodeo::default(), false, false);
    if module.node_collapse.num_pairs() != 0
        || !module.dae_system.noise_sources.is_empty()
        || !module.dae_system.small_signal_parameters.is_empty()
        || !module.intern.lim_state.is_empty()
        || module
            .dae_system
            .residual
            .iter()
            .any(|r| r.resist_lim_rhs != mir::F_ZERO || r.react_lim_rhs != mir::F_ZERO)
    {
        return Err("collapse/noise/small-signal/limiting requires a separate capability".into());
    }
    let mut params = info.params.keys().copied().collect::<Vec<_>>();
    params.sort_by_key(|p| p.name(&db));
    if params.len() > 128
        || params.iter().any(|p| p.name(&db) == "temperature_K" || p.ty(&db) != Type::Real)
    {
        return Err(
            "unsupported parameter layout; require real parameters and reserved temperature_K"
                .into(),
        );
    }
    let nodes = module
        .dae_system
        .unknowns
        .raw
        .iter()
        .map(|k| match *k {
            SimUnknownKind::KirchoffLaw(n) if n.discipline(&db).name(&db) == "electrical" => {
                Ok(n.name(&db).to_string())
            }
            _ => Err("only electrical node unknowns supported".to_string()),
        })
        .collect::<Result<Vec<_>>>()?;
    if nodes.is_empty() || nodes.len() > 64 {
        return Err("node layout must contain 1..64 unknowns".into());
    }
    let mut cache =
        module.init.cached_vals.iter().map(|(v, c)| (usize::from(*c), *v)).collect::<Vec<_>>();
    cache.sort_by_key(|(i, _)| *i);
    if cache.iter().enumerate().any(|(i, (c, _))| i != *c) {
        return Err("non-contiguous cache slots".into());
    }
    for (kind, _) in &module.init.intern.outputs {
        if !matches!(
            kind,
            PlaceKind::Param(_)
                | PlaceKind::ParamMin(_)
                | PlaceKind::ParamMax(_)
                | PlaceKind::Var(_)
        ) {
            return Err(format!("unsupported initialization output: {kind:?}"));
        }
    }
    let init_inputs = inputs(&module.init.func, &module.init.intern, &module, &params)?;
    let mut eval_inputs = inputs(&module.eval, &module.intern, &module, &params)?;
    for i in 0..cache.len() {
        eval_inputs.insert(module.intern.params.len() + i, json!({"kind":"cache","index":i}));
    }
    let roots = module
        .dae_system
        .residual
        .iter()
        .map(|r| r.resist)
        .chain(module.dae_system.residual.iter().map(|r| r.react))
        .collect::<Vec<_>>();
    let preparation = function(
        &module.init.func,
        &init_inputs,
        &cache.iter().map(|(_, v)| *v).collect::<Vec<_>>(),
    )?;
    let evaluation = function(&module.eval, &eval_inputs, &roots)?;
    let jacobian=module.dae_system.jacobian.iter().map(|e|json!({"row":usize::from(e.row),"column":usize::from(e.col),"f":id(e.resist),"q":id(e.react)})).collect::<Vec<_>>();
    let mut names = params.iter().map(|p| p.name(&db)).collect::<Vec<_>>();
    names.push("temperature_K".into());
    let document = json!({"schema":"circuit-numerical-ir","version":1,"profile":"stateless_electrical_fq_v1","model":info.module.name(&db),"variables":nodes,"parameters":names,
        "producer":{"name":"openvaf-numerical-ir","revision":option_env!("OPENVAF_IR_REVISION").unwrap_or("unrecorded"),"source_sha256":format!("{:x}",Sha256::digest(&source))},
        "preparation":preparation,"evaluation":evaluation,"jacobian":jacobian});
    std::fs::write(
        output,
        serde_json::to_string_pretty(&document).map_err(|e| e.to_string())? + "\n",
    )
    .map_err(|e| e.to_string())
}
fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    if args.len() == 2 && args[1] == "--version" {
        println!(
            "openvaf-numerical-ir {} schema=1 revision={}",
            env!("CARGO_PKG_VERSION"),
            option_env!("OPENVAF_IR_REVISION").unwrap_or("unrecorded")
        );
        return;
    }
    if args.len() != 3 {
        eprintln!("usage: openvaf-numerical-ir INPUT.va OUTPUT.json");
        std::process::exit(2);
    }
    if let Err(e) = export(Path::new(&args[1]), Path::new(&args[2])) {
        eprintln!("numerical export rejected: {e}");
        std::process::exit(1);
    }
}
