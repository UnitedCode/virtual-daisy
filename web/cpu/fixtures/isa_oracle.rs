#![no_std]
#![no_main]
use core::arch::asm;
#[repr(C)]
union Vector{stack:u32,handler:extern "C" fn()->!}
#[used]
#[unsafe(link_section=".vector_table")]
static VECTORS:[Vector;2]=[Vector{stack:0x20020000},Vector{handler:Reset}];
unsafe fn call(op:u32,arg:u32)->u32{let mut r0=op;unsafe{asm!("bkpt #0xab",inout("r0")r0,in("r1")arg,options(nostack));}r0}
fn exit(code:u32)->!{let args=[0x20026,code];unsafe{call(0x20,args.as_ptr()as u32);}loop{}}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo)->!{exit(1)}
macro_rules! floating{($op:literal,$a:expr,$b:expr)=>{{let value:u32;let flags:u32;unsafe{asm!("vmov s1, {a}","vmov s2, {b}","vmsr fpscr, {zero}",$op,"vmov {value}, s0","vmrs {flags}, fpscr",a=in(reg)$a,b=in(reg)$b,zero=in(reg)0u32,value=out(reg)value,flags=out(reg)flags,out("s0")_,out("s1")_,out("s2")_,options(nostack));}(value,flags)}};}
#[unsafe(no_mangle)]
extern "C" fn Reset()->!{
 unsafe{core::ptr::write_volatile(0xe000ed88 as *mut u32,0x00f00000);asm!("dsb","isb",options(nostack));}
 let specials=[0u32,0x80000000,0x3f800000,0xbf800000,0x00800000,0x7f7fffff,1,0x80000001,0x7f800000,0x7fc12345];
 let mut output=[0u32;5*4*210];let mut cursor=0;let mut seed=0x56443312u32;
 for i in 0..210{
  let a=if i<10{specials[i]}else{seed^=seed<<13;seed^=seed>>17;seed^=seed<<5;seed};
  seed^=seed<<13;seed^=seed>>17;seed^=seed<<5;let b=if i<10{specials[(i*3+1)%10]}else{seed};
  let results=[floating!("vadd.f32 s0,s1,s2",a,b),floating!("vsub.f32 s0,s1,s2",a,b),floating!("vmul.f32 s0,s1,s2",a,b),floating!("vdiv.f32 s0,s1,s2",a,b)];
  for (op,(value,flags)) in results.into_iter().enumerate(){output[cursor..cursor+5].copy_from_slice(&[a,b,op as u32,value,flags]);cursor+=5;}
 }
 let hex=b"0123456789abcdef";
 for record in output.chunks_exact(5) {
  let mut line=[0u8;42];let mut n=0;
  for word in record {for byte in word.to_le_bytes(){line[n]=hex[(byte>>4)as usize];line[n+1]=hex[(byte&15)as usize];n+=2;}}
  line[40]=b'\n';unsafe{call(4,line.as_ptr()as u32);}
 }
 exit(0)
}
