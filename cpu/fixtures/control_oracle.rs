#![no_std]
#![no_main]
use core::{
    arch::{asm, global_asm},
    ptr::write_volatile,
};
const CODE: u32 = 0x00200000;
const SENTINEL: u32 = 0xdeadbeef;
#[repr(C)]
#[derive(Clone, Copy)]
union Vector {
    stack: u32,
    handler: extern "C" fn() -> !,
}
#[used]
#[unsafe(link_section = ".vector_table")]
static VECTORS: [Vector; 16] = {
    let mut vectors = [Vector {
        handler: unexpected,
    }; 16];
    vectors[0] = Vector { stack: 0x20020000 };
    vectors[1] = Vector { handler: Reset };
    vectors
};
global_asm!(
    r#"
    .syntax unified
    .thumb
    .section .text.run_control,"ax",%progbits
    .global run_control
    .thumb_func
run_control:
    push {{r4, lr}}
    mov r4, r0
    movw r0, #0xbeef
    movt r0, #0xdead
    msr APSR_nzcvqg, r1
    blx r4
    mrs r1, APSR
    pop {{r4, pc}}
"#
);
unsafe extern "C" {
    fn run_control(code: u32, flags: u32) -> u64;
}
unsafe fn semihost(operation: u32, argument: u32) {
    unsafe {
        asm!("bkpt #0xab", inout("r0") operation => _, in("r1") argument, options(nostack));
    }
}
fn exit(status: u32) -> ! {
    let arguments = [0x20026, status];
    unsafe {
        semihost(0x20, arguments.as_ptr() as u32);
    }
    loop {}
}
extern "C" fn unexpected() -> ! {
    exit(2)
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    exit(1)
}
fn emit(record: [u32; 5]) {
    let hex = b"0123456789abcdef";
    let mut line = [0u8; 47];
    line[..5].copy_from_slice(b"CTRL:");
    let mut at = 5;
    for word in record {
        for byte in word.to_le_bytes() {
            line[at] = hex[usize::from(byte >> 4)];
            line[at + 1] = hex[usize::from(byte & 15)];
            at += 2;
        }
    }
    line[45] = b'\n';
    unsafe {
        semihost(4, line.as_ptr() as u32);
    }
}
unsafe fn halfword(at: u32, value: u16) {
    unsafe {
        write_volatile(at as *mut u16, value);
    }
}
unsafe fn instruction(at: u32, opcode: u32, width: u32) {
    unsafe {
        if width == 4 {
            halfword(at, (opcode >> 16) as u16);
            halfword(at + 2, opcode as u16);
        } else {
            halfword(at, opcode as u16);
        }
    }
}
unsafe fn return_pc(at: u32) {
    unsafe {
        halfword(at, 0x4678);
        halfword(at + 2, 0x4770);
    } // MOV R0,PC; BX LR.
}
unsafe fn capture(flags: u32) -> (u32, u32) {
    unsafe {
        asm!("dsb", "isb", options(nostack));
    }
    let result = unsafe { run_control(CODE | 1, flags) };
    (result as u32, (result >> 32) as u32)
}
fn branch(condition: u32, offset: i32) -> u32 {
    let immediate = offset as u32 & 0x1fffff;
    0xf0008000
        | (condition << 22)
        | ((immediate & 0x100000) << 6)
        | ((immediate & 0x80000) >> 8)
        | ((immediate & 0x40000) >> 5)
        | ((immediate & 0x3f000) << 4)
        | ((immediate & 0xffe) >> 1)
}
fn hint(opcode: u32, width: u32) {
    for inside_it in [false, true] {
        for pattern in 0..16u32 {
            let flags = pattern << 28;
            let start = CODE + if inside_it { 2 } else { 0 };
            unsafe {
                if inside_it {
                    halfword(CODE, 0xbf0c);
                } // ITE EQ.
                instruction(start, opcode, width);
                return_pc(start + width);
            }
            let (pc, final_flags) = unsafe { capture(flags) };
            let expected = if inside_it && flags & (1 << 30) != 0 {
                SENTINEL
            } else {
                start + width + 4
            };
            assert_eq!(pc, expected);
            assert_eq!(final_flags, flags);
            emit([
                width | if inside_it { 0x10 } else { 0 },
                opcode,
                flags,
                pc,
                final_flags,
            ]);
        }
    }
}
#[unsafe(no_mangle)]
extern "C" fn Reset() -> ! {
    for condition in 0..14u32 {
        for offset in [-1048576, -524288, -262144, -8, 8, 262144, 524288, 1048574] {
            let opcode = branch(condition, offset);
            let target = CODE.wrapping_add(4).wrapping_add(offset as u32);
            unsafe {
                instruction(CODE, opcode, 4);
                return_pc(CODE + 4);
                return_pc(target);
            }
            for pattern in 0..16u32 {
                let flags = pattern << 28;
                let (pc, final_flags) = unsafe { capture(flags) };
                assert!(pc == CODE + 8 || pc == target + 4);
                assert_eq!(final_flags, flags);
                emit([0, opcode, flags, pc, final_flags]);
            }
        }
    }
    for option in 5..16u32 {
        hint(0xbf00 | (option << 4), 2);
    }
    for option in 5..240u32 {
        hint(0xf3af8000 | option, 4);
    }
    exit(0)
}
