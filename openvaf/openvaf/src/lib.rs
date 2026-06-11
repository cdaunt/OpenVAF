// Re-export the correct llvm-sys version based on feature flags
#[cfg(feature = "llvm18")]
extern crate llvm_sys_181 as llvm_sys;
#[cfg(feature = "llvm19")]
extern crate llvm_sys_191 as llvm_sys;
#[cfg(feature = "llvm20")]
extern crate llvm_sys_201 as llvm_sys;
#[cfg(feature = "llvm21")]
extern crate llvm_sys_211 as llvm_sys;

use std::fs::{create_dir_all, remove_file};
use std::io::Write;
use std::time::Instant;

use anyhow::{Context, Result};
use basedb::diagnostics::{ConsoleSink, DiagnosticSink};
pub use basedb::lints::{builtin as builtin_lints, LintLevel};
use basedb::BaseDB;
use camino::Utf8PathBuf;
use hir::CompilationDB;
use linker::link;
pub use llvm_sys::target_machine::LLVMCodeGenOptLevel;
use mir_llvm::LLVMBackend;
pub use paths::AbsPathBuf;
use lasso::Rodeo;
use sim_back::{collect_modules, print_intern, print_module, write_json};
use sim_back::CompiledModule as SimCompiledModule;
pub use target::host_triple;
pub use target::spec::{get_target_names, Target};
use termcolor::{Color, ColorChoice, ColorSpec, StandardStream, WriteColor};

mod cache;

#[derive(Debug, Clone)]
pub enum CompilationDestination {
    Path { lib_file: Utf8PathBuf },
    Cache { cache_dir: Utf8PathBuf },
}

pub enum CompilationTermination {
    Compiled { lib_file: Utf8PathBuf },
    FatalDiagnostic,
}

#[derive(Debug, Clone)]
pub struct Opts {
    pub dry_run: bool,
    pub defines: Vec<String>,
    pub codegen_opts: Vec<String>,
    pub lints: Vec<(String, LintLevel)>,
    pub input: Utf8PathBuf,
    pub output: CompilationDestination,
    pub include: Vec<AbsPathBuf>,
    pub opt_lvl: LLVMCodeGenOptLevel,
    pub target: Target,
    pub target_cpu: String,
    pub dump_mir: bool,
    pub dump_unopt_mir: bool,
    pub dump_ir: bool,
    pub dump_unopt_ir: bool,
    pub dump_unopt_json: bool,
    pub dump_unopt_json_with_split: bool,
}
/// Emit JSON (same schema as `--dump-json`) from the raw unoptimized split:
/// no ADCE, no SCCP, no GVN — preserves the full pre-optimization structure.
pub fn dump_unopt_json(opts: &Opts) -> Result<CompilationTermination> {
    let input =
        opts.input.canonicalize().with_context(|| format!("failed to resolve {}", opts.input))?;
    let input = AbsPathBuf::assert(input);
    let db = CompilationDB::new_fs(input, &opts.include, &opts.defines, &opts.lints)?;

    let module_infos =
        if let Some(m) = collect_modules(&db, false, &mut ConsoleSink::new(&db)) {
            m
        } else {
            return Ok(CompilationTermination::FatalDiagnostic);
        };

    let mut literals = Rodeo::new();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    write!(out, "[")?;
    for (i, module_info) in module_infos.iter().enumerate() {
        if i > 0 {
            write!(out, ",")?;
        }
        // skip_value_opts=true, run_adce=false → raw split, no optimization
        let compiled =
            SimCompiledModule::new_with_opts(&db, module_info, &mut literals, false, false, true, false);
        write_json(&compiled, &db, &literals, &mut out)?;
    }
    writeln!(out, "]")?;

    Ok(CompilationTermination::Compiled { lib_file: Utf8PathBuf::default() })
}

/// Emit JSON (same schema as `--dump-json`) from the ADCE-only refined split:
/// no SCCP/GVN/phi-collapse, but ADCE + simplify_cfg_no_phi_merge are applied
/// to prune dead cache slots while preserving 2-edge phi structure.
pub fn dump_unopt_json_with_split(opts: &Opts) -> Result<CompilationTermination> {
    let input =
        opts.input.canonicalize().with_context(|| format!("failed to resolve {}", opts.input))?;
    let input = AbsPathBuf::assert(input);
    let db = CompilationDB::new_fs(input, &opts.include, &opts.defines, &opts.lints)?;

    let module_infos =
        if let Some(m) = collect_modules(&db, false, &mut ConsoleSink::new(&db)) {
            m
        } else {
            return Ok(CompilationTermination::FatalDiagnostic);
        };

    let mut literals = Rodeo::new();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    write!(out, "[")?;
    for (i, module_info) in module_infos.iter().enumerate() {
        if i > 0 {
            write!(out, ",")?;
        }
        // skip_value_opts=true, run_adce=true → ADCE-only refined split
        let compiled =
            SimCompiledModule::new_with_opts(&db, module_info, &mut literals, false, false, true, true);
        write_json(&compiled, &db, &literals, &mut out)?;
    }
    writeln!(out, "]")?;

    Ok(CompilationTermination::Compiled { lib_file: Utf8PathBuf::default() })
}

pub fn dump_json(opts: &Opts) -> Result<CompilationTermination> {
    let input =
        opts.input.canonicalize().with_context(|| format!("failed to resolve {}", opts.input))?;
    let input = AbsPathBuf::assert(input);
    let db = CompilationDB::new_fs(input, &opts.include, &opts.defines, &opts.lints)?;

    let module_infos =
        if let Some(m) = collect_modules(&db, false, &mut ConsoleSink::new(&db)) {
            m
        } else {
            return Ok(CompilationTermination::FatalDiagnostic);
        };

    let mut literals = Rodeo::new();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    // One module per VA `module` declaration; wrap in JSON array.
    write!(out, "[")?;
    for (i, module_info) in module_infos.iter().enumerate() {
        if i > 0 {
            write!(out, ",")?;
        }
        let compiled = SimCompiledModule::new(&db, module_info, &mut literals, false, false);
        write_json(&compiled, &db, &literals, &mut out)?;
    }
    writeln!(out, "]")?;

    Ok(CompilationTermination::Compiled { lib_file: Utf8PathBuf::default() })
}

pub fn expand(opts: &Opts) -> Result<CompilationTermination> {
    let start = Instant::now();

    let input =
        opts.input.canonicalize().with_context(|| format!("failed to resolve {}", opts.input))?;
    let input = AbsPathBuf::assert(input);
    let db = CompilationDB::new_fs(input, &opts.include, &opts.defines, &opts.lints)?;
    let cu = db.compilation_unit();

    let preprocess = cu.preprocess(&db);
    for token in preprocess.ts.iter() {
        let span = token.span.to_file_span(&preprocess.sm);
        let text = db.file_text(span.file).unwrap();
        match token.kind {
            tokens::parser::SyntaxKind::COMMENT => {
                // Block comments are ok
                // Line comments should be dumped with a newline
                if !text[span.range].starts_with("/*") {
                    println!("{}", &text[span.range])
                } else {
                    print!("{}", &text[span.range])
                }
            }
            _ => {
                // Add a space after each token
                print!("{} ", &text[span.range])
            }
        };
    }
    println!();

    let mut sink = ConsoleSink::new(&db);
    sink.add_diagnostics(&*preprocess.diagnostics, cu.root_file(), &db);

    if sink.summary(&opts.input.file_name().unwrap()) {
        return Ok(CompilationTermination::FatalDiagnostic);
    }

    let seconds = Instant::elapsed(&start).as_secs_f64();
    let mut stderr = StandardStream::stderr(ColorChoice::Auto);
    stderr.set_color(ColorSpec::new().set_fg(Some(Color::Green)).set_bold(true))?;
    write!(&mut stderr, "Finished")?;
    stderr.set_color(&ColorSpec::new())?;
    writeln!(&mut stderr, " preprocessing {} in {:.2}s", opts.input.file_name().unwrap(), seconds)?;

    Ok(CompilationTermination::Compiled { lib_file: Utf8PathBuf::default() })
}

pub fn compile(opts: &Opts) -> Result<CompilationTermination> {
    let start = Instant::now();

    let input =
        opts.input.canonicalize().with_context(|| format!("failed to resolve {}", opts.input))?;
    let input = AbsPathBuf::assert(input);
    let db = CompilationDB::new_fs(input, &opts.include, &opts.defines, &opts.lints)?;

    let lib_file = match &opts.output {
        CompilationDestination::Cache { cache_dir } => {
            let file_name = cache::file_name(&db, opts);
            let lib_file = cache_dir.join(file_name);
            if cfg!(not(debug_assertions)) && lib_file.exists() {
                return Ok(CompilationTermination::Compiled { lib_file });
            }
            create_dir_all(cache_dir).context("failed to create cache directory")?;
            lib_file
        }
        CompilationDestination::Path { lib_file } => lib_file.clone(),
    };

    // Lowering of natures from AST into HIR happens here
    let modules = if let Some(modules) = collect_modules(&db, false, &mut ConsoleSink::new(&db)) {
        modules
    } else {
        return Ok(CompilationTermination::FatalDiagnostic);
    };

    let back = LLVMBackend::new(&opts.codegen_opts, &opts.target, opts.target_cpu.clone(), &[]);
    if opts.dry_run {
        return Ok(CompilationTermination::Compiled { lib_file });
    }
    // HIR lowering into MIR happens here
    let (paths, compiled_modules, literals) = osdi::compile(
        &db,
        &modules,
        &lib_file,
        &opts.target,
        &back,
        true,
        opts.opt_lvl,
        opts.dump_mir,
        opts.dump_unopt_mir,
        opts.dump_ir,
        opts.dump_unopt_ir,
    );

    // Dump natures, disciplines, and their attributes
    /*
    let cu = db.compilation_unit();
    let nda_table = db.nda_table(cu.root_file());
    for (i, nature) in nda_table.natures.iter_enumerated() {
        println!("{:?}: {}", i, nature.name);
        println!("  parent: {:?}", nature.parent);
        println!("  ddt: {:?}", nature.ddt_nature);
        println!("  idt: {:?}", nature.idt_nature);
        for ndx in nature.attr_range.clone() {
            let attr = &nda_table.attributes[ndx];
            println!("  attr {:?} = {:?}", attr.name, attr.value);
        }
    }
    for (i, discipline) in nda_table.disciplines.iter_enumerated() {
        println!("{:?}: {}", i, discipline.name);
        println!("  flow: {:?}", discipline.flow_nature);
        println!("  potential: {:?}", discipline.potential_nature);
        for ndx in discipline.attr_range.clone() {
            let attr = &nda_table.attributes[ndx];
            println!("  attr {:?} = {:?}", attr.name, attr.value);
        }
    }
    */

    // Dump MIR of compiled modules
    if opts.dump_mir || opts.dump_unopt_mir {
        let cu = db.compilation_unit();
        println!("Compilation unit: {}", cu.name(&db));
        println!("");

        println!("Literals:");
        for (k, v) in literals.iter() {
            println!("  {:?} -> '{}'", k, v);
        }
        println!("");

        for (module, cmodule) in modules.iter().zip(compiled_modules.iter()) {
            print_module("  ", &db, &module, &cmodule.dae_system, &cmodule.init);
            println!("");

            println!("Model setup HIR interner of {}", module.module.name(&db));
            print_intern("  ", &db, &cmodule.model_param_intern);
            println!("");

            println!("Instance setup HIR interner of {}", module.module.name(&db));
            print_intern("  ", &db, &cmodule.init.intern);
            println!("");

            println!("Evaluation HIR interner of {}", module.module.name(&db));
            print_intern("  ", &db, &cmodule.intern);
            println!("");
        }
    }

    // TODO configure linker
    link(None, &opts.target, lib_file.as_ref(), |linker| {
        for path in &paths {
            linker.add_object(path);
        }
    })?;

    for obj_file in paths {
        remove_file(obj_file).context("failed to delete intermediate compile artifact")?;
    }

    let seconds = Instant::elapsed(&start).as_secs_f64();
    let mut stderr = StandardStream::stderr(ColorChoice::Auto);
    stderr.set_color(ColorSpec::new().set_fg(Some(Color::Green)).set_bold(true))?;
    write!(&mut stderr, "Finished")?;
    stderr.set_color(&ColorSpec::new())?;
    writeln!(&mut stderr, " building {} in {:.2}s", opts.input.file_name().unwrap(), seconds)?;

    Ok(CompilationTermination::Compiled { lib_file })
}
