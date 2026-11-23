//! PSR mask/alias behavior, including unprivileged writes, executed by QEMU.
use core::arch::asm;
core::arch::global_asm!(
    r#"
.syntax unified
.thumb
.section .text.restore_privilege,"ax",%progbits
.global restore_privilege
.thumb_func
restore_privilege:
movs r0, #0
msr CONTROL, r0
isb
bx lr
"#
);
extern "C" {
    pub(super) fn restore_privilege() -> !;
}
#[repr(align(4))]
struct Code([u16; 4]);
static mut CODE: Code = Code([0; 4]);
fn emit(opcode: u32, input: u32, control: u32, unprivileged: u32) {
    let target = unsafe { core::ptr::addr_of_mut!(CODE.0).cast::<u16>() };
    let apsr: u32;
    let xpsr: u32;
    let epsr: u32;
    unsafe {
        target.write_volatile((opcode >> 16) as u16);
        target.add(1).write_volatile(opcode as u16);
        target.add(2).write_volatile(0x4770);
        if unprivileged != 0 {
            asm!("msr CONTROL, {value}","isb",value=in(reg)1u32,options(nostack));
        }
        asm!("dsb","isb","mov r1, {input}","msr APSR_nzcvqg, {control}","blx {target}",
            "mrs {apsr}, APSR","mrs {xpsr}, XPSR","mrs {epsr}, EPSR",
            input=in(reg)input,control=in(reg)control,target=in(reg)(target as u32|1),
            apsr=out(reg)apsr,xpsr=out(reg)xpsr,epsr=out(reg)epsr,out("r1")_,out("lr")_,options(nostack));
        if unprivileged != 0 {
            asm!("svc #0");
        }
    }
    let words = [
        opcode,
        input,
        control,
        unprivileged,
        apsr,
        xpsr,
        epsr,
        0x52535041,
    ];
    let hex = b"0123456789abcdef";
    let mut line = [0u8; 66];
    for (i, byte) in words.into_iter().flat_map(u32::to_le_bytes).enumerate() {
        line[2 * i] = hex[(byte >> 4) as usize];
        line[2 * i + 1] = hex[(byte & 15) as usize];
    }
    line[64] = b'\n';
    unsafe {
        super::call(4, line.as_ptr() as u32);
    }
}
pub(super) fn run() {
    for unprivileged in [0u32, 1] {
        for sysm in 0..4 {
            for mask in 1..=3 {
                let opcode = 0xf3818000 | (mask << 10) | sysm;
                for input in [
                    0, 0xffffffff, 0xa80a0000, 0x50050000, 0x000f0000, 0xf8000000, 0x01000000,
                    0x00ff03ff,
                ] {
                    for control in [0xa00a0000, 0x50050000, 0xa80a0000, 0x58050000] {
                        emit(opcode, input, control, unprivileged);
                    }
                }
            }
        }
    }
}
