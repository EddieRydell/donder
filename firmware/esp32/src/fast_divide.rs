//! Correctly rounded f32 division using the ESP32 FPU division-assist
//! instructions, the same sequence as the Xtensa toolchain libgcc. This strong
//! symbol replaces compiler-builtins' software routine (about 200 cycles) for
//! every f32 division; it is bit-identical, including NaN, infinity and subnormals.
#[unsafe(no_mangle)]
#[unsafe(naked)]
pub extern "C" fn __divsf3(_a: f32, _b: f32) -> f32 {
    core::arch::naked_asm!(
        "entry a1, 16",
        "wfr f1, a2",
        "wfr f2, a3",
        "div0.s f3, f2",
        "nexp01.s f4, f2",
        "const.s f5, 1",
        "maddn.s f5, f4, f3",
        "mov.s f6, f3",
        "mov.s f7, f2",
        "nexp01.s f2, f1",
        "maddn.s f6, f5, f6",
        "const.s f5, 1",
        "const.s f0, 0",
        "neg.s f8, f2",
        "maddn.s f5, f4, f6",
        "maddn.s f0, f8, f3",
        "mkdadj.s f7, f1",
        "maddn.s f6, f5, f6",
        "maddn.s f8, f4, f0",
        "const.s f3, 1",
        "maddn.s f3, f4, f6",
        "maddn.s f0, f8, f6",
        "neg.s f2, f2",
        "maddn.s f6, f3, f6",
        "maddn.s f2, f4, f0",
        "addexpm.s f0, f7",
        "addexp.s f6, f7",
        "divn.s f0, f2, f6",
        "rfr a2, f0",
        "retw.n",
    )
}
