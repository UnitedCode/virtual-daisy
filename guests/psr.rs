//! PSR aliases, masked writes, unprivileged access and exception preservation.
#![no_std]
#![no_main]
use core::{
    arch::{asm, global_asm},
    ptr::{read_volatile, write_volatile},
};
const RESULTS: *mut u32 = 0x20018000 as *mut u32;
// PSR_CASES
global_asm!(
    r#"
.syntax unified
.thumb
.section .vectors,"a",%progbits
.word 0x2001f000, probe_main
.rept 9
.word probe_unexpected
.endr
.word restore_privilege
.rept 4
.word probe_unexpected
.endr
.section .text.restore_privilege,"ax",%progbits
.global restore_privilege
.thumb_func
restore_privilege:
movs r0, #0
msr APSR_g, r0
msr CONTROL, r0
isb
bx lr
.section .text.probe_unexpected,"ax",%progbits
.global probe_unexpected
.thumb_func
probe_unexpected:
ldr r1, =0x20018000
mrs r0, ipsr
str r0, [r1, #124]
ldr r0, =0xe000ed28
ldr r2, [r0]
str r2, [r1, #100]
ldr r2, [r0, #4]
str r2, [r1, #104]
1: wfi
b 1b
"#
);
#[inline(never)]
unsafe fn run() -> bool {
    let code = 0x7000usize;
    write_volatile((code + 4) as *mut u16, 0x4770);
    let mut hash = 0xcbf29ce484222325u64;
    for (index, [opcode, input, control, unpriv, apsr, xpsr, epsr]) in CASES.into_iter().enumerate()
    {
        write_volatile(code as *mut u16, (opcode >> 16) as u16);
        write_volatile((code + 2) as *mut u16, opcode as u16);
        let (a, x, e, after, mode): (u32, u32, u32, u32, u32);
        asm!("dsb","isb","msr CONTROL, {unpriv}","isb","mov r1, {input}","msr APSR_nzcvqg, {control}","blx {target}",
            "mrs {a}, APSR","mrs {x}, XPSR","mrs {e}, EPSR","svc #0","mrs {after}, APSR","mrs {mode}, CONTROL",
            unpriv=in(reg)unpriv,input=in(reg)input,control=in(reg)control,target=in(reg)(code|1),
            a=out(reg)a,x=out(reg)x,e=out(reg)e,after=out(reg)after,mode=out(reg)mode,out("r1")_,out("lr")_);
        let actual = [a, x, e, after, mode & 1];
        if actual != [apsr, xpsr, epsr, apsr, 0] {
            write_volatile(RESULTS, 0xbad0f119);
            write_volatile(RESULTS.add(1), index as u32);
            for (i, value) in actual.into_iter().enumerate() {
                write_volatile(RESULTS.add(2 + i), value);
            }
            return false;
        }
        for word in actual {
            for byte in word.to_le_bytes() {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
        }
    }
    write_volatile(RESULTS.add(27), CASES.len() as u32);
    write_volatile(RESULTS.add(28), hash as u32);
    write_volatile(RESULTS.add(29), (hash >> 32) as u32);
    write_volatile(RESULTS.add(30), 1);
    true
}
#[no_mangle]
pub unsafe extern "C" fn probe_main() -> ! {
    for i in 0..32 {
        write_volatile(RESULTS.add(i), 0);
    }
    write_volatile(0xe000ed88 as *mut u32, 0x00f00000);
    asm!("dsb", "isb", options(nostack));
    if run() && read_volatile(RESULTS.add(31)) == 0 {
        write_volatile(RESULTS, 0xab00f119);
    }
    loop {
        asm!("wfi", options(nostack));
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
