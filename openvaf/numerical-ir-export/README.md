# Data-only numerical interchange exporter

`cargo build --locked --release -p openvaf-numerical-ir` builds the companion CLI.
It uses the compiler's typed model lowering, emitting no Rust or other host
language fragments. It has no dependency on BOSDI or Circulox. This compiler
extension is distributed under GPL-3.0-only with the compiler source.

```sh
openvaf-numerical-ir model.va model.ir.json
```

The artifact identifies `schema: circuit-numerical-ir`, `version: 1`, and
`profile: stateless_electrical_fq_v1`. Value and block IDs are opaque artifact-local
integers, not serialized compiler types. Operations have semantic names (`add`,
`multiply`, `less`, etc.); constants carry exact 16-digit hexadecimal binary64
bits. Values bind explicit parameters, temperature, unknown differences or
preparation outputs. Programs contain explicit basic blocks, parallel phi nodes,
ordinary instructions, and jump/Boolean-branch/return terminators. Evaluation
outputs are F then Q; Jacobian entries specify local row/column and separate
resistive/reactive value roots. Preparation outputs are ordered cache slots.

Every declared parameter is explicitly supplied. `temperature_K` is the final
parameter plane; multiplicity is fixed to one and terminals are connected.
Default/domain admission is not implemented by this raw interchange profile.
Collapse, noise, small-signal-only inputs, limiting, temporal/history callbacks,
non-real parameters and non-electrical node unknowns reject. Consumers must
independently validate schema/profile, bindings, references, types, control flow,
and supported preparation types before generating/executing code. Do not infer
an unsupported capability from a numerically zero value.

The source digest identifies the root Verilog-A file. Compiler-provided include
content is identified by the pinned compiler source; arbitrary external include
dependency closure is not yet captured. Producer revision is embedded from
`OPENVAF_IR_REVISION` at compiler build time (otherwise `unrecorded`). A producer
identity or source digest is provenance, not an executable trust certificate.

The existing compiler-artifacts workflow builds, packages and relocation-tests
this companion alongside `openvaf-r`. It uploads candidates; it does not publish
releases. The data format does not itself grant rights to model source or copied
compiler/runtime implementation. Keep compiler and model licenses explicit.
