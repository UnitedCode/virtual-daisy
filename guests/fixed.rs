//! Fixed conversions with independent QEMU results and all populated registers.
#![no_std]
#![no_main]
use core::{arch::{asm,global_asm},ptr::{read_volatile,write_volatile}};
const RESULTS:*mut u32=0x20018800 as *mut u32;
// FIXED_CASES
global_asm!(r#"
.syntax unified
.thumb
.section .vectors,"a",%progbits
.word 0x2001f000, probe_main
.rept 14
.word probe_unexpected
.endr
.section .text.probe_unexpected,"ax",%progbits
.global probe_unexpected
.thumb_func
probe_unexpected:
ldr r1, =0x20018800
mrs r0, ipsr
str r0, [r1, #124]
ldr r0, =0xe000ed28
ldr r2, [r0]
str r2, [r1, #100]
ldr r2, [r0, #4]
str r2, [r1, #104]
1: wfi
b 1b
"#);

#[inline(never)]
unsafe fn matrix()->bool {
    let code=0x7000usize;
    write_volatile((code+4) as *mut u16,0x4770);
    let mut count=0;let mut hash=0xcbf29ce484222325u64;
    for (case,[base,il,ih,control,lo,hi,status]) in CASES.into_iter().enumerate() {
        let double=base&(1<<8)!=0;
        for register in 0..if double {16} else {32} {
            let opcode=(base&!0x0040f000)|if double {register<<12} else {(register>>1)<<12|(register&1)<<22};
            let start=if double {register*2} else {register} as usize;
            let mut before:[u32;32]=core::array::from_fn(|i|0xdeadbeef^(i as u32*0x01010101));
            before[start]=il;if double {before[start+1]=ih;}
            let mut expected=before;expected[start]=lo;if double {expected[start+1]=hi;}
            let mut after=[0u32;32];let actual_status:u32;
            write_volatile(code as *mut u16,(opcode>>16) as u16);
            write_volatile((code+2) as *mut u16,opcode as u16);
            asm!("dsb","isb","vldmia {input}, {{s0-s31}}","vmsr fpscr, {control}",
                "blx {target}","vstmia {output}, {{s0-s31}}","vmrs {status}, fpscr",
                input=in(reg)before.as_ptr(),output=in(reg)after.as_mut_ptr(),control=in(reg)control,
                target=in(reg)(code|1),status=out(reg)actual_status,out("lr")_,
                out("d0")_,out("d1")_,out("d2")_,out("d3")_,out("d4")_,out("d5")_,out("d6")_,out("d7")_,
                out("d8")_,out("d9")_,out("d10")_,out("d11")_,out("d12")_,out("d13")_,out("d14")_,out("d15")_,options(nostack));
            for i in 0..33 {
                let (actual,wanted)=if i==32 {(actual_status,status)} else {(after[i],expected[i])};
                if actual!=wanted {
                    write_volatile(RESULTS,0xbad0f017);write_volatile(RESULTS.add(1),case as u32);
                    write_volatile(RESULTS.add(2),opcode);write_volatile(RESULTS.add(3),i as u32);
                    write_volatile(RESULTS.add(4),actual);return false;
                }
                for byte in actual.to_le_bytes() {hash=(hash^u64::from(byte)).wrapping_mul(0x100000001b3);}
            }
            count+=1;
        }
    }
    write_volatile(RESULTS.add(27),count);write_volatile(RESULTS.add(28),hash as u32);
    write_volatile(RESULTS.add(29),(hash>>32) as u32);write_volatile(RESULTS.add(30),1);
    true
}
#[no_mangle]
pub unsafe extern "C" fn probe_main()->! {
    for i in 0..32 {write_volatile(RESULTS.add(i),0);}
    write_volatile(0xe000ed88 as *mut u32,0x00f00000);asm!("dsb","isb",options(nostack));
    if matrix() && read_volatile(RESULTS.add(31))==0 {write_volatile(RESULTS,0xab00f017);}
    loop {asm!("wfi",options(nostack));}
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo)->! {loop {}}
