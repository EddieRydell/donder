/* Keep the interpreter and graph evaluation in instruction RAM. This is board
 * placement, not a second runtime implementation. Mangled names select the
 * module's functions (_RNvNt), methods (_RNvMs), closures (_RNC) and trait
 * methods (_RNvX) independently of the crate disambiguator; drop glue (_RINv)
 * stays in flash. */
*(.literal._RNvNt*2vm5strip* .text._RNvNt*2vm5strip*)
*(.literal._RNvMs*2vm5strip* .text._RNvMs*2vm5strip*)
*(.literal._RNC*2vm5strip* .text._RNC*2vm5strip*)
/* Language helpers the interpreter calls per strip or pixel. The color
 * selectors match the Fn::call shims (_RNvY) of the kernels it passes. */
*(.literal.*8sampling21sample_gradient_stops* .text.*8sampling21sample_gradient_stops*)
*(.literal.*8sampling12float_binary* .text.*8sampling12float_binary*)
*(.literal.*8sampling14curve_crossing* .text.*8sampling14curve_crossing*)
*(.literal.*8sampling13query_seconds* .text.*8sampling13query_seconds*)
*(.literal.*8sampling14query_progress* .text.*8sampling14query_progress*)
*(.literal.*8sampling11scale_color* .text.*8sampling11scale_color*)
*(.literal.*8sampling15multiply_colors* .text.*8sampling15multiply_colors*)
*(.literal.*6values28sample_time_from_seconds_f32* .text.*6values28sample_time_from_seconds_f32*)
/* Graph traversal runs once per strip; flash placement put about a tenth of
 * Stanford's samples behind the flash cache. */
*(.literal._RNvNt*10evaluation* .text._RNvNt*10evaluation*)
*(.literal._RNvMs*10evaluation* .text._RNvMs*10evaluation*)
*(.literal._RNvX*10evaluation* .text._RNvX*10evaluation*)
/* Pixel lookup runs per pixel of nested and sectioned strips. */
*(.literal.*12TargetPixels5pixel* .text.*12TargetPixels5pixel*)
*(.literal.*16PreparedSections5pixel* .text.*16PreparedSections5pixel*)

/* Float division (hardware-assisted on this board) is hot in pixel code. */
*(.literal.__divsf3 .text.__divsf3)
