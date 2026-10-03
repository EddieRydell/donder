/* Keep the shared interpreter in instruction RAM. This is board placement,
 * not a second runtime implementation. Match the Rust symbol's stable method
 * suffix, including its literals, independently of the crate disambiguator. */
*(.literal.*2Vm*3run* .text.*2Vm*3run*)
/* Shared-interpreter code generation can outline register/resource helpers.
 * Keep those hot calls beside dispatch instead of making placement depend on
 * the compiler's inlining decisions. */
*(.literal.*2Vm*3new* .text.*2Vm*3new*)
*(.literal.*2Vm*9copy_slot* .text.*2Vm*9copy_slot*)
*(.literal.*2Vm*9set_array* .text.*2Vm*9set_array*)
*(.literal.*2Vm*9set_color* .text.*2Vm*9set_color*)
/* Shared lane scheduling runs only at branches/joins. Keep one native helper
 * instead of duplicating the cohort scan in every conditional instruction. */
*(.literal.*5lanes*4Flow* .text.*5lanes*4Flow*)
/* Sparse masks share one software bit search; dense lanes just advance. */
*(.literal.*5lanes13skip_inactive* .text.*5lanes13skip_inactive*)
*(.literal.*11VmRegisters17broadcast_numeric* .text.*11VmRegisters17broadcast_numeric*)
*(.literal.*3dsl2vm12sample_curve* .text.*3dsl2vm12sample_curve*)
*(.literal.*3dsl2vm15sample_gradient* .text.*3dsl2vm15sample_gradient*)
/* Keep the effect entry beside the interpreter: its flash placement produced
 * a measured layout-sensitive slowdown even while both VMs were in IRAM. */
*(.literal.*16SampleProgramExt6sample .text.*16SampleProgramExt6sample)
*(.literal.*19sample_signal_graph* .text.*19sample_signal_graph*)
*(.literal.*18sample_layer_frame* .text.*18sample_layer_frame*)
/* Block traversal replaces repeated scalar source queries on eligible graphs. */
*(.literal.*10evaluation6blocks* .text.*10evaluation6blocks*)

/* Software float division remains hot in interrupted-PC samples. */
*(.literal.*8___divsf3 .text.*8___divsf3)
*(.literal.__divsf3 .text.__divsf3)
