/* Keep the interpreter and graph evaluation in instruction RAM. This is board
 * placement, not a second runtime implementation. Mangled names select the
 * module's functions (_RNvNt), methods (_RNvMs) and trait methods (_RNvX)
 * independently of the crate disambiguator; drop glue (_RINv) stays in flash. */
*(.literal._RNvNt*2vm5batch* .text._RNvNt*2vm5batch*)
*(.literal._RNvMs*2vm5batchNtB*_5Batch* .text._RNvMs*2vm5batchNtB*_5Batch*)
*(.literal._RNvMs*2vm5batchNtB*_6Parked* .text._RNvMs*2vm5batchNtB*_6Parked*)
*(.literal._RNvMs*2vm5batchNtB*_10References* .text._RNvMs*2vm5batchNtB*_10References*)
/* Helpers called per lane, found in flash by interrupted-PC samples. */
*(.literal.*3dsl2vm12sample_curve* .text.*3dsl2vm12sample_curve*)
*(.literal.*3dsl2vm15sample_gradient* .text.*3dsl2vm15sample_gradient*)
*(.literal.*8sampling10max_colors* .text.*8sampling10max_colors*)
*(.literal.*8sampling9color_hue* .text.*8sampling9color_hue*)
*(.literal.*8sampling16color_saturation* .text.*8sampling16color_saturation*)
*(.literal.*8sampling15color_intensity* .text.*8sampling15color_intensity*)
*(.literal.*13CurveRegister8crossing* .text.*13CurveRegister8crossing*)
*(.literal.*3dsl2vm10prev_index* .text.*3dsl2vm10prev_index*)
*(.literal.*3dsl2vm13previous_mark* .text.*3dsl2vm13previous_mark*)
*(.literal.*12ArrayStorage7release* .text.*12ArrayStorage7release*)
*(.literal.*9drop_glue*12RuntimeValue* .text.*9drop_glue*12RuntimeValue*)
*(.literal._RNC*2vm5batch* .text._RNC*2vm5batch*)
*(.literal.*4core3f32f5clamp* .text.*4core3f32f5clamp*)
*(.literal.*4libm4math5floor* .text.*4libm4math5floor*)
*(.literal.*9micromath5float3sin* .text.*9micromath5float3sin*)
/* Graph traversal runs once per run of pixels; flash placement put about a
 * tenth of Stanford's samples behind the flash cache. */
*(.literal._RNvNt*10evaluation* .text._RNvNt*10evaluation*)
*(.literal._RNvMs*10evaluation* .text._RNvMs*10evaluation*)
*(.literal._RNvX*10evaluation* .text._RNvX*10evaluation*)
/* Target pixel lookup runs per lane of nested runs. */
*(.literal._RNvMs*7targets* .text._RNvMs*7targets*)

/* Float division (hardware-assisted on this board) is hot in pixel code. */
*(.literal.*8___divsf3 .text.*8___divsf3)
*(.literal.__divsf3 .text.__divsf3)
